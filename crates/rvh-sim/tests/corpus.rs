//! Run the corpus programs and check what they compute.
//!
//! These are end-to-end: source text goes through the parser, the semantic
//! layer, the assembler, and the machine. A regression in any layer shows up
//! here as a wrong register.

use std::path::Path;

use rvh_isa::Target;

fn run(name: &str) -> rvh_sim::Machine {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus")
        .join(name);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));

    let tree = rvh_syntax::parse(&source);
    assert!(
        tree.diagnostics().is_empty(),
        "{name}: {:?}",
        tree.diagnostics()
    );

    let program = rvh_sem::resolve(&tree, &Target::parse("rv32i_zicsr").expect("target"));
    assert!(
        program.diagnostics.is_empty(),
        "{name}: {:?}",
        program.diagnostics
    );

    let (image, diagnostics) = rvh_asm::assemble(&program);
    assert!(diagnostics.is_empty(), "{name}: {diagnostics:?}");

    let mut machine = rvh_sim::Machine::new();
    machine.load(&image);
    machine.run(1_000_000);
    machine
}

#[test]
fn loop_sum_computes_55() {
    let machine = run("001_loop_sum.s");
    assert_eq!(machine.reg(10), 55);
    assert!(matches!(
        machine.halt(),
        Some(rvh_sim::HaltReason::Ebreak { .. })
    ));
}

#[test]
fn factorial_computes_120() {
    let machine = run("002_factorial.s");
    assert_eq!(machine.reg(10), 120);
    assert_eq!(machine.reg(2), rvh_sim::STACK_TOP, "sp must be restored");
}

#[test]
fn hello_prints_to_the_console() {
    let machine = run("000_hello_mmio.s");
    assert_eq!(machine.output_text(), "Hi\n");
}

#[test]
fn numeric_labels_land_on_the_right_instruction() {
    let machine = run("003_numeric_labels.s");
    // The `j 1f` skips `addi a0, a0, 100`, so a0 ends as 1, not 101.
    assert_eq!(machine.reg(10), 1);
}

#[test]
fn la_reaches_the_data_section() {
    let machine = run("004_directives.s");
    // .text is 4 words, so .data starts at 16 and `message` is its first byte.
    assert_eq!(machine.reg(5), 16);
    assert_eq!(machine.reg(6), 4);
}

#[test]
fn a_file_that_declares_its_target_uses_it() {
    // `mul` is only legal because the file declares M with `.option arch, +m`.
    let machine = run("006_arch_directives.s");
    assert_eq!(machine.reg(12), 42);

    // `.attribute arch` declares the whole file.
    let machine = run("007_arch_attribute.s");
    assert_eq!(machine.reg(12), 42);
}

#[test]
fn the_double_registers_hold_sixty_four_bits() {
    // 1.5 + 2.25 = 3.75 and 1.5 * 2.25 + 1.5 = 4.875, read back as halves.
    let machine = run("011_double.s");
    assert_eq!(machine.reg(10), 0x400e_0000);
    assert_eq!(machine.reg(11), 0x4013_8000);
    assert_eq!(machine.reg(12), 4);
}

#[test]
fn the_float_registers_carry_their_bit_patterns() {
    // 1.5 + 2.25 = 3.75 and 2.25 - 1.5 = 0.75, read back through `fmv.x.w`.
    let machine = run("010_float.s");
    assert_eq!(machine.reg(10), 0x4070_0000);
    assert_eq!(machine.reg(11), 0x3f40_0000);
    // The store and the load round-trip through memory.
    assert_eq!(machine.freg(3), machine.freg(2));
}

#[test]
fn the_compressed_instructions_run() {
    // Six two-byte instructions: 6 + 7 = 13, +1 = 14, <<1 = 28, then c.mv.
    let machine = run("009_compressed.s");
    assert_eq!(machine.reg(10), 28);
    assert_eq!(machine.reg(12), 28);
}

#[test]
fn the_embedded_base_runs() {
    let machine = run("008_rv32e.s");
    assert_eq!(machine.reg(12), 7);
}

#[test]
fn trap_csr_installs_the_handler_address() {
    let machine = run("005_trap_csr.s");
    assert_ne!(machine.reg(5), 0);
    assert!(matches!(
        machine.halt(),
        Some(rvh_sim::HaltReason::Ebreak { .. })
    ));
}

#[test]
fn undo_replays_a_corpus_program_back_to_the_start() {
    let mut machine = run("001_loop_sum.s");
    let steps = machine.steps();
    assert!(steps > 0);
    let mut undone = 0;
    while machine.undo() {
        undone += 1;
    }
    assert_eq!(undone, steps);
    assert_eq!(machine.pc(), 0);
    assert_eq!(machine.reg(10), 0);
    // `sp` is set before the program runs, so it is not part of the history.
    assert_eq!(machine.reg(2), rvh_sim::STACK_TOP);
}
