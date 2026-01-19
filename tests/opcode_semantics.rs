use std::sync::{Arc, Mutex};

use chip8_runtime::display::display::SCALE;
use chip8_runtime::proc::proc::Proc;
use chip8_runtime::shared_memory::shared_memory::SharedMemory;

fn new_headless_proc() -> Proc<'static> {
    let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
    let mem_ref: &'static mut Arc<Mutex<SharedMemory>> = Box::leak(Box::new(mem));
    Proc::new_headless(mem_ref).unwrap()
}

fn new_headless_proc_with_pages(pages: u16) -> Proc<'static> {
    let mem = Arc::new(Mutex::new(SharedMemory::new().unwrap()));
    let mem_ref: &'static mut Arc<Mutex<SharedMemory>> = Box::leak(Box::new(mem));
    Proc::new_headless_with_pages(mem_ref, pages).unwrap()
}

fn write_opcode(proc: &mut Proc<'_>, addr: u16, opcode: u16) {
    let hi = (opcode >> 8) as u8;
    let lo = opcode as u8;
    let data = vec![hi, lo];
    proc.write_bytes(addr as u32, &data).unwrap();
}

fn write_byte(proc: &mut Proc<'_>, addr: u16, value: u8) {
    proc.write_u8(addr as u32, value).unwrap();
}

fn exec_opcode(proc: &mut Proc<'_>, opcode: u16) {
    let pc = proc.regs.PC;
    write_opcode(proc, pc, opcode);
    proc.step();
}

fn count_on_pixels(proc: &Proc<'_>) -> usize {
    proc.display.buf.iter().filter(|&&p| p != 0).count()
}

#[test]
fn opcode_00e0_clears_screen() {
    let mut proc = new_headless_proc();
    proc.display.buf.iter_mut().for_each(|p| *p = 0xFFFFFF);
    exec_opcode(&mut proc, 0x00E0);
    assert!(proc.display.buf.iter().all(|&p| p == 0));
    assert_eq!(proc.regs.PC, 0x202);
}

#[test]
fn opcode_00ee_returns_to_caller() {
    let mut proc = new_headless_proc();
    proc.regs.PC = 0x200;
    exec_opcode(&mut proc, 0x2300);
    assert_eq!(proc.regs.PC, 0x300);
    assert_eq!(proc.regs.SP, 0xFA2);

    let ret_hi = proc.read_u8(proc.regs.SP as u32).unwrap();
    let ret_lo = proc.read_u8((proc.regs.SP + 1) as u32).unwrap();
    assert_eq!(ret_hi, 0x02);
    assert_eq!(ret_lo, 0x02);

    write_opcode(&mut proc, 0x300, 0x00EE);
    proc.step();
    assert_eq!(proc.regs.PC, 0x202);
    assert_eq!(proc.regs.SP, 0xFA0);
}

#[test]
fn opcode_0nnn_is_ignored() {
    let mut proc = new_headless_proc();
    proc.regs.PC = 0x200;
    exec_opcode(&mut proc, 0x0123);
    assert_eq!(proc.regs.PC, 0x202);
}

#[test]
fn opcode_1nnn_jumps() {
    let mut proc = new_headless_proc();
    exec_opcode(&mut proc, 0x1456);
    assert_eq!(proc.regs.PC, 0x456);
}

#[test]
fn opcode_2nnn_calls() {
    let mut proc = new_headless_proc();
    exec_opcode(&mut proc, 0x2345);
    assert_eq!(proc.regs.PC, 0x345);
    assert_eq!(proc.regs.SP, 0xFA2);
}

#[test]
fn opcode_3xkk_skips_on_equal() {
    let mut proc = new_headless_proc();
    proc.regs.V[1] = 0x42;
    exec_opcode(&mut proc, 0x3142);
    assert_eq!(proc.regs.PC, 0x204);
}

#[test]
fn opcode_3xkk_no_skip_on_inequal() {
    let mut proc = new_headless_proc();
    proc.regs.V[1] = 0x41;
    exec_opcode(&mut proc, 0x3142);
    assert_eq!(proc.regs.PC, 0x202);
}

#[test]
fn opcode_4xkk_skips_on_inequal() {
    let mut proc = new_headless_proc();
    proc.regs.V[2] = 0x10;
    exec_opcode(&mut proc, 0x4211);
    assert_eq!(proc.regs.PC, 0x204);
}

#[test]
fn opcode_5xy0_skips_on_equal() {
    let mut proc = new_headless_proc();
    proc.regs.V[2] = 0x55;
    proc.regs.V[3] = 0x55;
    exec_opcode(&mut proc, 0x5230);
    assert_eq!(proc.regs.PC, 0x204);
}

#[test]
fn opcode_6xkk_loads_register() {
    let mut proc = new_headless_proc();
    exec_opcode(&mut proc, 0x63AA);
    assert_eq!(proc.regs.V[3], 0xAA);
    assert_eq!(proc.regs.PC, 0x202);
}

#[test]
fn opcode_7xkk_adds_immediate_with_wrap() {
    let mut proc = new_headless_proc();
    proc.regs.V[4] = 0xFF;
    exec_opcode(&mut proc, 0x7401);
    assert_eq!(proc.regs.V[4], 0x00);
}

#[test]
fn opcode_8xy4_sets_carry() {
    let mut proc = new_headless_proc();
    proc.regs.V[1] = 200;
    proc.regs.V[2] = 100;
    exec_opcode(&mut proc, 0x8124);
    assert_eq!(proc.regs.V[1], 44);
    assert_eq!(proc.regs.V[0xF], 1);
}

#[test]
fn opcode_8xy5_sets_borrow_flag() {
    let mut proc = new_headless_proc();
    proc.regs.V[1] = 5;
    proc.regs.V[2] = 5;
    exec_opcode(&mut proc, 0x8125);
    assert_eq!(proc.regs.V[1], 0);
    assert_eq!(proc.regs.V[0xF], 0);
}

#[test]
fn opcode_8xy7_uses_strict_borrow_rule() {
    let mut proc = new_headless_proc();
    proc.regs.V[1] = 5;
    proc.regs.V[2] = 5;
    exec_opcode(&mut proc, 0x8127);
    assert_eq!(proc.regs.V[1], 0);
    assert_eq!(proc.regs.V[0xF], 0);
}

#[test]
fn opcode_8xy6_shifts_vx_right() {
    let mut proc = new_headless_proc();
    proc.regs.V[1] = 0b0000_0011;
    exec_opcode(&mut proc, 0x8106);
    assert_eq!(proc.regs.V[1], 0b0000_0001);
    assert_eq!(proc.regs.V[0xF], 1);
}

#[test]
fn opcode_8xye_shifts_vx_left() {
    let mut proc = new_headless_proc();
    proc.regs.V[1] = 0b1000_0001;
    exec_opcode(&mut proc, 0x810E);
    assert_eq!(proc.regs.V[1], 0b0000_0010);
    assert_eq!(proc.regs.V[0xF], 1);
}

#[test]
fn opcode_9xy0_skips_on_inequal() {
    let mut proc = new_headless_proc();
    proc.regs.V[1] = 1;
    proc.regs.V[2] = 2;
    exec_opcode(&mut proc, 0x9120);
    assert_eq!(proc.regs.PC, 0x204);
}

#[test]
fn opcode_annn_sets_i() {
    let mut proc = new_headless_proc();
    exec_opcode(&mut proc, 0xA123);
    assert_eq!(proc.regs.I, 0x123);
}

#[test]
fn opcode_bnnn_adds_v0_to_jump() {
    let mut proc = new_headless_proc();
    proc.regs.V[0] = 5;
    exec_opcode(&mut proc, 0xB200);
    assert_eq!(proc.regs.PC, 0x205);
}

#[test]
fn opcode_cxkk_masks_random() {
    let mut proc = new_headless_proc();
    exec_opcode(&mut proc, 0xC30F);
    assert_eq!(proc.regs.V[3] & 0xF0, 0x00);
}

#[test]
fn opcode_dxyn_draws_and_collides() {
    let mut proc = new_headless_proc();
    proc.regs.I = 0x300;
    write_byte(&mut proc, 0x300, 0xF0);
    proc.regs.V[0] = 0;
    proc.regs.V[1] = 0;

    exec_opcode(&mut proc, 0xD011);
    assert_eq!(proc.regs.PC, 0x202);
    assert_eq!(count_on_pixels(&proc), 4 * SCALE * SCALE);

    proc.regs.PC = 0x200;
    exec_opcode(&mut proc, 0xD011);
    assert_eq!(count_on_pixels(&proc), 0);
    assert_eq!(proc.regs.V[0xF], 1);
}

#[test]
fn opcode_ex9e_skips_if_key_pressed() {
    let mut proc = new_headless_proc();
    proc.regs.V[2] = 0xA;
    proc.display.key_down[0xA] = true;
    exec_opcode(&mut proc, 0xE29E);
    assert_eq!(proc.regs.PC, 0x204);
}

#[test]
fn opcode_exa1_skips_if_key_not_pressed() {
    let mut proc = new_headless_proc();
    proc.regs.V[2] = 0xA;
    proc.display.key_down[0xA] = false;
    exec_opcode(&mut proc, 0xE2A1);
    assert_eq!(proc.regs.PC, 0x204);
}

#[test]
fn opcode_fx07_reads_delay_timer() {
    let mut proc = new_headless_proc();
    proc.regs.DT = 7;
    exec_opcode(&mut proc, 0xF207);
    assert_eq!(proc.regs.V[2], 7);
}

#[test]
fn opcode_fx0a_blocks_until_key() {
    let mut proc = new_headless_proc();
    exec_opcode(&mut proc, 0xF20A);
    assert_eq!(proc.regs.PC, 0x200);

    proc.display.key_down[0x5] = true;
    exec_opcode(&mut proc, 0xF20A);
    assert_eq!(proc.regs.V[2], 0x5);
    assert_eq!(proc.regs.PC, 0x202);
}

#[test]
fn opcode_fx15_sets_delay_timer() {
    let mut proc = new_headless_proc();
    proc.regs.V[3] = 9;
    exec_opcode(&mut proc, 0xF315);
    assert_eq!(proc.regs.DT, 9);
}

#[test]
fn opcode_fx18_sets_sound_timer() {
    let mut proc = new_headless_proc();
    proc.regs.V[3] = 9;
    exec_opcode(&mut proc, 0xF318);
    assert_eq!(proc.regs.ST, 9);
}

#[test]
fn opcode_fx1e_adds_to_i() {
    let mut proc = new_headless_proc();
    proc.regs.I = 0x100;
    proc.regs.V[1] = 0x10;
    exec_opcode(&mut proc, 0xF11E);
    assert_eq!(proc.regs.I, 0x110);
}

#[test]
fn opcode_fx29_sets_sprite_address() {
    let mut proc = new_headless_proc();
    proc.regs.V[4] = 0xA;
    exec_opcode(&mut proc, 0xF429);
    assert_eq!(proc.regs.I, 0x32);
}

#[test]
fn opcode_fx33_stores_bcd() {
    let mut proc = new_headless_proc();
    proc.regs.I = 0x300;
    proc.regs.V[7] = 137;
    exec_opcode(&mut proc, 0xF733);

    let data = vec![
        proc.read_u8(0x300).unwrap(),
        proc.read_u8(0x301).unwrap(),
        proc.read_u8(0x302).unwrap(),
    ];
    assert_eq!(data, vec![1, 3, 7]);
}

#[test]
fn opcode_fx55_stores_registers_and_increments_i() {
    let mut proc = new_headless_proc();
    proc.regs.I = 0x300;
    proc.regs.V[0] = 1;
    proc.regs.V[1] = 2;
    proc.regs.V[2] = 3;
    exec_opcode(&mut proc, 0xF255);

    let data = vec![
        proc.read_u8(0x300).unwrap(),
        proc.read_u8(0x301).unwrap(),
        proc.read_u8(0x302).unwrap(),
    ];
    assert_eq!(data, vec![1, 2, 3]);
    assert_eq!(proc.regs.I, 0x303);
}

#[test]
fn opcode_fx65_loads_registers_and_increments_i() {
    let mut proc = new_headless_proc();
    proc.regs.I = 0x300;
    write_byte(&mut proc, 0x300, 0xAA);
    write_byte(&mut proc, 0x301, 0x55);
    write_byte(&mut proc, 0x302, 0xCC);
    exec_opcode(&mut proc, 0xF265);

    assert_eq!(proc.regs.V[0], 0xAA);
    assert_eq!(proc.regs.V[1], 0x55);
    assert_eq!(proc.regs.V[2], 0xCC);
    assert_eq!(proc.regs.I, 0x303);
}

#[test]
fn virtual_translation_spans_pages() {
    let mut proc = new_headless_proc_with_pages(2);
    // Codex generated: verify distinct bytes across the 0x0FFF/0x1000 boundary.
    proc.write_u8(0x0FFF, 0xAA).unwrap();
    proc.write_u8(0x1000, 0x55).unwrap();

    assert_eq!(proc.read_u8(0x0FFF).unwrap(), 0xAA);
    assert_eq!(proc.read_u8(0x1000).unwrap(), 0x55);
}
