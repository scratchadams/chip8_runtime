use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(err) = run() {
        eprintln!("c8asm: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).peekable();
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        if arg == "-o" || arg == "--out" {
            let Some(path) = args.next() else {
                return Err("-o/--out requires a path".into());
            };
            output = Some(PathBuf::from(path));
            continue;
        }
        if input.is_none() {
            input = Some(PathBuf::from(arg));
            continue;
        }
        if output.is_none() {
            output = Some(PathBuf::from(arg));
            continue;
        }
        return Err(format!("unexpected argument '{arg}'"));
    }

    let input = input.ok_or_else(|| "missing input file".to_string())?;
    let output = output.unwrap_or_else(|| default_output_path(&input));

    let source = fs::read_to_string(&input)
        .map_err(|err| format!("failed to read {}: {err}", input.display()))?;

    let mut assembler = Assembler::new();
    let rom = assembler.assemble(&source)?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    fs::write(&output, rom)
        .map_err(|err| format!("failed to write {}: {err}", output.display()))?;

    Ok(())
}

fn default_output_path(input: &Path) -> PathBuf {
    let mut out = input.to_path_buf();
    out.set_extension("ch8");
    out
}

#[derive(Debug, Clone)]
enum Token {
    Ident(String),
    Number(u16),
    Str(String),
    Sym(&'static str),
}

#[derive(Debug, Clone)]
enum Expr {
    Num(u16),
    Label(String),
}

#[derive(Debug, Clone, Copy)]
struct Reg(u8);

#[derive(Debug, Clone)]
enum Operand {
    Imm(Expr),
    Reg(Reg),
}

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
}

#[derive(Debug, Clone)]
enum Instr {
    Ret,
    Jump(Expr),
    Call(Expr),
    LoadI(Expr),
    AddI(Reg),
    Load(Reg, Operand),
    Add(Reg, Operand),
    SubImm(Reg, Expr),
    Save(Reg),
    LoadMem(Reg),
    If {
        left: Reg,
        op: CmpOp,
        right: Operand,
        target: Expr,
    },
}

#[derive(Debug, Clone)]
enum Stmt {
    Byte(Vec<Expr>),
    Word(Vec<Expr>),
    Zero(u16),
    Ascii(Vec<u8>),
    Sys(Expr),
    Instr(Instr),
}

#[derive(Debug, Clone)]
struct StmtLine {
    addr: u16,
    line_no: usize,
    stmt: Stmt,
}

#[derive(Debug, Clone)]
struct SectionState {
    name: String,
    pc: u16,
}

#[derive(Debug, Clone)]
enum BlockKind {
    Section(String),
    Label,
}

struct Assembler {
    labels: HashMap<String, u16>,
    section_end: HashMap<String, u16>,
    lines: Vec<StmtLine>,
}

impl Assembler {
    fn new() -> Self {
        Self {
            labels: HashMap::new(),
            section_end: HashMap::new(),
            lines: Vec::new(),
        }
    }

    fn assemble(&mut self, source: &str) -> Result<Vec<u8>, String> {
        self.first_pass(source)?;
        self.second_pass()
    }

    fn first_pass(&mut self, source: &str) -> Result<(), String> {
        let mut current: Option<SectionState> = None;
        let mut block_stack: Vec<BlockKind> = Vec::new();

        for (idx, raw_line) in source.lines().enumerate() {
            let line_no = idx + 1;
            let line = strip_comments(raw_line)?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let tokens = tokenize(line).map_err(|err| format!("line {line_no}: {err}"))?;
            if tokens.is_empty() {
                continue;
            }

            if tokens.len() == 1 && matches!(tokens[0], Token::Sym("}")) {
                match block_stack.pop() {
                    Some(BlockKind::Label) => {}
                    Some(BlockKind::Section(name)) => {
                        let Some(state) = current.take() else {
                            return Err(format!("line {line_no}: stray section close"));
                        };
                        if state.name != name {
                            return Err(format!("line {line_no}: mismatched section close"));
                        }
                        self.section_end.insert(state.name, state.pc);
                    }
                    None => return Err(format!("line {line_no}: unmatched '}}'")),
                }
                continue;
            }

            if let Token::Ident(keyword) = &tokens[0] {
                if keyword == "section" {
                    let (name, addr) = parse_section_header(&tokens)
                        .map_err(|err| format!("line {line_no}: {err}"))?;
                    let addr = match addr {
                        Some(value) => value,
                        None => self
                            .section_end
                            .get(&name)
                            .copied()
                            .ok_or_else(|| {
                                format!("line {line_no}: section '{name}' has no prior address")
                            })?,
                    };

                    current = Some(SectionState { name: name.clone(), pc: addr });
                    block_stack.push(BlockKind::Section(name));
                    continue;
                }

                if keyword == "label" {
                    let name = parse_label_header(&tokens)
                        .map_err(|err| format!("line {line_no}: {err}"))?;
                    let state = current.as_ref().ok_or_else(|| {
                        format!("line {line_no}: label outside of a section")
                    })?;
                    self.insert_label(&name, state.pc, line_no)?;
                    block_stack.push(BlockKind::Label);
                    continue;
                }
            }

            if let Some((label, rest)) = parse_inline_label(&tokens) {
                let state = current.as_ref().ok_or_else(|| {
                    format!("line {line_no}: label outside of a section")
                })?;
                self.insert_label(&label, state.pc, line_no)?;
                if rest.is_empty() {
                    continue;
                }
                let stmt = parse_stmt(&rest)
                    .map_err(|err| format!("line {line_no}: {err}"))?;
                let size = stmt_size(&stmt)?;
                let state = current.as_mut().expect("section state missing");
                self.lines.push(StmtLine { addr: state.pc, line_no, stmt });
                state.pc = checked_add(state.pc, size, line_no)?;
                continue;
            }

            let stmt = parse_stmt(&tokens)
                .map_err(|err| format!("line {line_no}: {err}"))?;
            let size = stmt_size(&stmt)?;
            let state = current.as_mut().ok_or_else(|| {
                format!("line {line_no}: statement outside of a section")
            })?;
            self.lines.push(StmtLine { addr: state.pc, line_no, stmt });
            state.pc = checked_add(state.pc, size, line_no)?;
        }

        if !block_stack.is_empty() {
            return Err("unclosed block(s) at end of file".into());
        }

        Ok(())
    }

    fn second_pass(&self) -> Result<Vec<u8>, String> {
        let mut max_end = 0u16;
        let mut min_addr = u16::MAX;

        for line in &self.lines {
            min_addr = min_addr.min(line.addr);
            let size = stmt_size(&line.stmt)?;
            let end = line.addr.checked_add(size).ok_or_else(|| {
                format!("line {}: address overflow", line.line_no)
            })?;
            max_end = max_end.max(end);
        }

        if min_addr == u16::MAX {
            return Ok(Vec::new());
        }

        if min_addr < 0x200 {
            return Err(format!(
                "ROM start {min_addr:#06x} is below 0x200; Chip-8 programs load at 0x200"
            ));
        }

        let base = 0x200u16;
        let total_len = max_end
            .checked_sub(base)
            .ok_or_else(|| "program ends before base address".to_string())? as usize;

        let mut output = vec![0u8; total_len];
        let mut written = vec![false; total_len];

        for line in &self.lines {
            let addr = line.addr;
            let bytes = self.emit_stmt(&line.stmt, line.line_no)?;
            let offset = addr
                .checked_sub(base)
                .ok_or_else(|| format!("line {}: address below base", line.line_no))? as usize;

            for (idx, byte) in bytes.into_iter().enumerate() {
                let pos = offset + idx;
                if pos >= output.len() {
                    return Err(format!(
                        "line {}: write past end of ROM (offset {pos})",
                        line.line_no
                    ));
                }
                if written[pos] {
                    return Err(format!(
                        "line {}: overlapping data at {:#06x}",
                        line.line_no,
                        addr + idx as u16
                    ));
                }
                output[pos] = byte;
                written[pos] = true;
            }
        }

        Ok(output)
    }

    fn emit_stmt(&self, stmt: &Stmt, line_no: usize) -> Result<Vec<u8>, String> {
        match stmt {
            Stmt::Byte(values) => {
                let mut out = Vec::new();
                for expr in values {
                    let val = self.resolve_expr(expr, line_no)?;
                    if val > 0xFF {
                        return Err(format!("line {line_no}: byte value {val:#06x} too large"));
                    }
                    out.push(val as u8);
                }
                Ok(out)
            }
            Stmt::Word(values) => {
                let mut out = Vec::new();
                for expr in values {
                    let val = self.resolve_expr(expr, line_no)?;
                    out.push((val >> 8) as u8);
                    out.push((val & 0xFF) as u8);
                }
                Ok(out)
            }
            Stmt::Zero(count) => Ok(vec![0u8; *count as usize]),
            Stmt::Ascii(bytes) => Ok(bytes.clone()),
            Stmt::Sys(expr) => {
                let val = self.resolve_expr(expr, line_no)?;
                if val & 0xF000 != 0 {
                    return Err(format!("line {line_no}: sys value {val:#06x} exceeds 0x0FFF"));
                }
                Ok(vec![(val >> 8) as u8, (val & 0xFF) as u8])
            }
            Stmt::Instr(instr) => self.emit_instr(instr, line_no),
        }
    }

    fn emit_instr(&self, instr: &Instr, line_no: usize) -> Result<Vec<u8>, String> {
        match instr {
            Instr::Ret => Ok(word(0x00EE)),
            Instr::Jump(expr) => {
                let addr = self.resolve_addr(expr, line_no)?;
                Ok(word(0x1000 | addr))
            }
            Instr::Call(expr) => {
                let addr = self.resolve_addr(expr, line_no)?;
                Ok(word(0x2000 | addr))
            }
            Instr::LoadI(expr) => {
                let addr = self.resolve_addr(expr, line_no)?;
                Ok(word(0xA000 | addr))
            }
            Instr::AddI(reg) => Ok(word(0xF01E | ((reg.0 as u16) << 8))),
            Instr::Load(reg, operand) => match operand {
                Operand::Imm(expr) => {
                    let val = self.resolve_byte(expr, line_no)?;
                    Ok(word(0x6000 | ((reg.0 as u16) << 8) | val as u16))
                }
                Operand::Reg(src) => Ok(word(
                    0x8000 | ((reg.0 as u16) << 8) | ((src.0 as u16) << 4),
                )),
            },
            Instr::Add(reg, operand) => match operand {
                Operand::Imm(expr) => {
                    let val = self.resolve_byte(expr, line_no)?;
                    Ok(word(0x7000 | ((reg.0 as u16) << 8) | val as u16))
                }
                Operand::Reg(src) => Ok(word(
                    0x8004 | ((reg.0 as u16) << 8) | ((src.0 as u16) << 4),
                )),
            },
            Instr::SubImm(reg, expr) => {
                let val = self.resolve_byte(expr, line_no)?;
                let delta = (0x100u16 - val as u16) & 0xFF;
                Ok(word(0x7000 | ((reg.0 as u16) << 8) | delta))
            }
            Instr::Save(reg) => Ok(word(0xF055 | ((reg.0 as u16) << 8))),
            Instr::LoadMem(reg) => Ok(word(0xF065 | ((reg.0 as u16) << 8))),
            Instr::If { left, op, right, target } => {
                let skip = match (op, right) {
                    (CmpOp::Eq, Operand::Imm(expr)) => {
                        let val = self.resolve_byte(expr, line_no)?;
                        0x4000 | ((left.0 as u16) << 8) | val as u16
                    }
                    (CmpOp::Ne, Operand::Imm(expr)) => {
                        let val = self.resolve_byte(expr, line_no)?;
                        0x3000 | ((left.0 as u16) << 8) | val as u16
                    }
                    (CmpOp::Eq, Operand::Reg(reg)) => {
                        0x9000 | ((left.0 as u16) << 8) | ((reg.0 as u16) << 4)
                    }
                    (CmpOp::Ne, Operand::Reg(reg)) => {
                        0x5000 | ((left.0 as u16) << 8) | ((reg.0 as u16) << 4)
                    }
                };
                let addr = self.resolve_addr(target, line_no)?;
                let jump = 0x1000 | addr;
                Ok(vec![
                    (skip >> 8) as u8,
                    (skip & 0xFF) as u8,
                    (jump >> 8) as u8,
                    (jump & 0xFF) as u8,
                ])
            }
        }
    }

    fn resolve_expr(&self, expr: &Expr, line_no: usize) -> Result<u16, String> {
        match expr {
            Expr::Num(val) => Ok(*val),
            Expr::Label(name) => self.labels.get(name).copied().ok_or_else(|| {
                format!("line {line_no}: unknown label '{name}'")
            }),
        }
    }

    fn resolve_byte(&self, expr: &Expr, line_no: usize) -> Result<u8, String> {
        let val = self.resolve_expr(expr, line_no)?;
        if val > 0xFF {
            return Err(format!("line {line_no}: value {val:#06x} does not fit in a byte"));
        }
        Ok(val as u8)
    }

    fn resolve_addr(&self, expr: &Expr, line_no: usize) -> Result<u16, String> {
        let addr = self.resolve_expr(expr, line_no)?;
        if addr > 0x0FFF {
            return Err(format!("line {line_no}: address {addr:#06x} exceeds 0x0FFF"));
        }
        Ok(addr)
    }

    fn insert_label(&mut self, name: &str, addr: u16, line_no: usize) -> Result<(), String> {
        if self.labels.contains_key(name) {
            return Err(format!("line {line_no}: duplicate label '{name}'"));
        }
        self.labels.insert(name.to_string(), addr);
        Ok(())
    }
}

fn strip_comments(line: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        if ch == '"' {
            out.push(ch);
            in_string = !in_string;
            continue;
        }
        if !in_string && (ch == ';' || ch == '#') {
            break;
        }
        out.push(ch);
    }

    if in_string {
        return Err("unterminated string literal".into());
    }

    Ok(out)
}

fn tokenize(line: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            let mut ident = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || c == '_' {
                    ident.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token::Ident(ident));
            continue;
        }

        if ch.is_ascii_digit() {
            let mut text = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_hexdigit() || c == 'x' || c == 'X' {
                    text.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            let value = parse_number(&text)?;
            tokens.push(Token::Number(value));
            continue;
        }

        if ch == '"' {
            chars.next();
            let mut buf = String::new();
            while let Some(c) = chars.next() {
                if c == '"' {
                    break;
                }
                if c == '\\' {
                    let Some(escaped) = chars.next() else {
                        return Err("unterminated string escape".into());
                    };
                    let mapped = match escaped {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '0' => '\0',
                        '"' => '"',
                        '\\' => '\\',
                        other => {
                            return Err(format!("unsupported escape \\{other}"));
                        }
                    };
                    buf.push(mapped);
                } else {
                    buf.push(c);
                }
            }
            tokens.push(Token::Str(buf));
            continue;
        }

        let two = {
            let mut iter = chars.clone();
            let first = iter.next();
            let second = iter.next();
            if let (Some(a), Some(b)) = (first, second) {
                let mut tmp = String::new();
                tmp.push(a);
                tmp.push(b);
                Some(tmp)
            } else {
                None
            }
        };

        if let Some(two) = two {
            let sym = match two.as_str() {
                ":=" => Some(":="),
                "+=" => Some("+="),
                "-=" => Some("-="),
                "==" => Some("=="),
                "!=" => Some("!="),
                _ => None,
            };
            if let Some(sym) = sym {
                chars.next();
                chars.next();
                tokens.push(Token::Sym(sym));
                continue;
            }
        }

        let sym = match ch {
            '{' => "{",
            '}' => "}",
            ':' => ":",
            '@' => "@",
            ',' => ",",
            _ => return Err(format!("unexpected character '{ch}'")),
        };
        chars.next();
        tokens.push(Token::Sym(sym));
    }

    Ok(tokens)
}

fn parse_number(text: &str) -> Result<u16, String> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).map_err(|_| format!("invalid hex literal '{text}'"))
    } else {
        text.parse::<u16>()
            .map_err(|_| format!("invalid number '{text}'"))
    }
}

fn parse_section_header(tokens: &[Token]) -> Result<(String, Option<u16>), String> {
    if tokens.len() < 3 {
        return Err("section header too short".into());
    }
    let name = match &tokens[1] {
        Token::Ident(val) => val.clone(),
        _ => return Err("expected section name".into()),
    };

    if matches!(tokens.get(2), Some(Token::Sym("{"))) {
        return Ok((name, None));
    }

    if tokens.len() < 5 {
        return Err("section header requires '@ <addr> {'".into());
    }
    if !matches!(tokens.get(2), Some(Token::Sym("@"))) {
        return Err("expected '@' after section name".into());
    }
    let addr = match tokens.get(3) {
        Some(Token::Number(val)) => *val,
        _ => return Err("expected address after '@'".into()),
    };
    if !matches!(tokens.get(4), Some(Token::Sym("{"))) {
        return Err("expected '{' after section header".into());
    }
    Ok((name, Some(addr)))
}

fn parse_label_header(tokens: &[Token]) -> Result<String, String> {
    if tokens.len() != 3 {
        return Err("label header must be 'label NAME {'".into());
    }
    let name = match &tokens[1] {
        Token::Ident(val) => val.clone(),
        _ => return Err("expected label name".into()),
    };
    if !matches!(tokens[2], Token::Sym("{")) {
        return Err("expected '{' after label name".into());
    }
    Ok(name)
}

fn parse_inline_label(tokens: &[Token]) -> Option<(String, Vec<Token>)> {
    if tokens.len() >= 2 {
        if let (Token::Ident(name), Token::Sym(":")) = (&tokens[0], &tokens[1]) {
            return Some((name.clone(), tokens[2..].to_vec()));
        }
    }
    None
}

fn parse_stmt(tokens: &[Token]) -> Result<Stmt, String> {
    if tokens.is_empty() {
        return Err("empty statement".into());
    }

    match &tokens[0] {
        Token::Ident(keyword) if keyword == "byte" => {
            if tokens.len() < 2 {
                return Err("byte requires at least one value".into());
            }
            let values = parse_expr_list(&tokens[1..])?;
            Ok(Stmt::Byte(values))
        }
        Token::Ident(keyword) if keyword == "word" => {
            if tokens.len() < 2 {
                return Err("word requires at least one value".into());
            }
            let values = parse_expr_list(&tokens[1..])?;
            Ok(Stmt::Word(values))
        }
        Token::Ident(keyword) if keyword == "zero" => {
            if tokens.len() != 2 {
                return Err("zero requires a single count".into());
            }
            let count = match &tokens[1] {
                Token::Number(val) => *val,
                _ => return Err("zero requires a numeric count".into()),
            };
            Ok(Stmt::Zero(count))
        }
        Token::Ident(keyword) if keyword == "ascii" => {
            if tokens.len() != 2 {
                return Err("ascii requires a single string literal".into());
            }
            let bytes = match &tokens[1] {
                Token::Str(val) => val.as_bytes().to_vec(),
                _ => return Err("ascii requires a string literal".into()),
            };
            Ok(Stmt::Ascii(bytes))
        }
        Token::Ident(keyword) if keyword == "sys" => {
            if tokens.len() != 2 {
                return Err("sys requires a single value".into());
            }
            let expr = parse_expr(&tokens[1])?;
            Ok(Stmt::Sys(expr))
        }
        _ => Ok(Stmt::Instr(parse_instr(tokens)?)),
    }
}

fn parse_expr_list(tokens: &[Token]) -> Result<Vec<Expr>, String> {
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < tokens.len() {
        let token = &tokens[idx];
        if matches!(token, Token::Sym(",")) {
            idx += 1;
            continue;
        }
        out.push(parse_expr(token)?);
        idx += 1;
    }
    Ok(out)
}

fn parse_expr(token: &Token) -> Result<Expr, String> {
    match token {
        Token::Number(val) => Ok(Expr::Num(*val)),
        Token::Ident(name) => Ok(Expr::Label(name.clone())),
        _ => Err("expected number or label".into()),
    }
}

fn parse_instr(tokens: &[Token]) -> Result<Instr, String> {
    match &tokens[0] {
        Token::Ident(keyword) if keyword == "return" => Ok(Instr::Ret),
        Token::Ident(keyword) if keyword == "jump" => {
            if tokens.len() != 2 {
                return Err("jump requires a target".into());
            }
            Ok(Instr::Jump(parse_expr(&tokens[1])?))
        }
        Token::Ident(keyword) if keyword == "call" => {
            if tokens.len() != 2 {
                return Err("call requires a target".into());
            }
            Ok(Instr::Call(parse_expr(&tokens[1])?))
        }
        Token::Ident(keyword) if keyword == "save" => {
            if tokens.len() != 2 {
                return Err("save requires a register".into());
            }
            Ok(Instr::Save(parse_reg(&tokens[1])?))
        }
        Token::Ident(keyword) if keyword == "load" => {
            if tokens.len() != 2 {
                return Err("load requires a register".into());
            }
            Ok(Instr::LoadMem(parse_reg(&tokens[1])?))
        }
        Token::Ident(keyword) if keyword == "if" => parse_if(tokens),
        Token::Ident(keyword) if keyword == "i" => parse_i_assign(tokens),
        Token::Ident(_) => parse_reg_instr(tokens),
        _ => Err("unrecognized instruction".into()),
    }
}

fn parse_if(tokens: &[Token]) -> Result<Instr, String> {
    if tokens.len() != 7 {
        return Err("if syntax is: if vX == vY/0xNN then jump label".into());
    }

    let left = parse_reg(&tokens[1])?;
    let op = match &tokens[2] {
        Token::Sym("==") => CmpOp::Eq,
        Token::Sym("!=") => CmpOp::Ne,
        _ => return Err("if requires '==' or '!='".into()),
    };

    let right = match &tokens[3] {
        Token::Ident(name) if is_reg(name) => Operand::Reg(parse_reg(&tokens[3])?),
        _ => Operand::Imm(parse_expr(&tokens[3])?),
    };

    if !matches!(&tokens[4], Token::Ident(word) if word == "then") {
        return Err("if requires 'then'".into());
    }
    if !matches!(&tokens[5], Token::Ident(word) if word == "jump") {
        return Err("if only supports 'then jump'".into());
    }

    let target = parse_expr(&tokens[6])?;
    Ok(Instr::If { left, op, right, target })
}

fn parse_i_assign(tokens: &[Token]) -> Result<Instr, String> {
    if tokens.len() != 3 {
        return Err("i assignment requires i := expr or i += vX".into());
    }
    match &tokens[1] {
        Token::Sym(":=") => Ok(Instr::LoadI(parse_expr(&tokens[2])?)),
        Token::Sym("+=") => Ok(Instr::AddI(parse_reg(&tokens[2])?)),
        _ => Err("i supports := or +=".into()),
    }
}

fn parse_reg_instr(tokens: &[Token]) -> Result<Instr, String> {
    if tokens.len() != 3 {
        return Err("register instruction must be 3 tokens".into());
    }
    let dst = parse_reg(&tokens[0])?;
    match &tokens[1] {
        Token::Sym(":=") => {
            let operand = if matches!(&tokens[2], Token::Ident(name) if is_reg(name)) {
                Operand::Reg(parse_reg(&tokens[2])?)
            } else {
                Operand::Imm(parse_expr(&tokens[2])?)
            };
            Ok(Instr::Load(dst, operand))
        }
        Token::Sym("+=") => {
            let operand = if matches!(&tokens[2], Token::Ident(name) if is_reg(name)) {
                Operand::Reg(parse_reg(&tokens[2])?)
            } else {
                Operand::Imm(parse_expr(&tokens[2])?)
            };
            Ok(Instr::Add(dst, operand))
        }
        Token::Sym("-=") => Ok(Instr::SubImm(dst, parse_expr(&tokens[2])?)),
        _ => Err("unsupported register operator".into()),
    }
}

fn parse_reg(token: &Token) -> Result<Reg, String> {
    match token {
        Token::Ident(name) if is_reg(name) => {
            let digit = name.chars().nth(1).unwrap();
            let value = digit.to_digit(16).ok_or_else(|| "invalid register".to_string())? as u8;
            Ok(Reg(value))
        }
        _ => Err("expected register like v0..vF".into()),
    }
}

fn is_reg(name: &str) -> bool {
    if name.len() != 2 {
        return false;
    }
    let mut chars = name.chars();
    let v = chars.next().unwrap();
    let digit = chars.next().unwrap();
    v == 'v' && matches!(digit, '0'..='9' | 'A'..='F')
}

fn stmt_size(stmt: &Stmt) -> Result<u16, String> {
    match stmt {
        Stmt::Byte(values) => Ok(values.len() as u16),
        Stmt::Word(values) => Ok(values.len() as u16 * 2),
        Stmt::Zero(count) => Ok(*count),
        Stmt::Ascii(bytes) => Ok(bytes.len() as u16),
        Stmt::Sys(_) => Ok(2),
        Stmt::Instr(instr) => Ok(match instr {
            Instr::If { .. } => 4,
            _ => 2,
        }),
    }
}

fn checked_add(base: u16, size: u16, line_no: usize) -> Result<u16, String> {
    base.checked_add(size)
        .ok_or_else(|| format!("line {line_no}: address overflow"))
}

fn word(op: u16) -> Vec<u8> {
    vec![(op >> 8) as u8, (op & 0xFF) as u8]
}
