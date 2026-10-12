//! The two TheNet X1J EPROM halves. They stay in flash rather than SRAM
//! (memory.x, `.flash_rodata`): `tnc2_core::Board::from_rom` copies them
//! onto the heap, so they are only read while a board is built.

const LOWER: &[u8] = include_bytes!("../../../rom/THENET1.X1J");
const UPPER: &[u8] = include_bytes!("../../../rom/THENET2.X1J");

#[unsafe(link_section = ".flash_rodata")]
static THENET1: [u8; LOWER.len()] = *include_bytes!("../../../rom/THENET1.X1J");
#[unsafe(link_section = ".flash_rodata")]
static THENET2: [u8; UPPER.len()] = *include_bytes!("../../../rom/THENET2.X1J");

/// The lower half (THENET1.X1J).
pub fn lower() -> &'static [u8] {
    &THENET1
}

/// The upper half (THENET2.X1J).
pub fn upper() -> &'static [u8] {
    &THENET2
}
