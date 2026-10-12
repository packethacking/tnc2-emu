//! The header picotool looks for to show a firmware's name and version
//! (`picotool info`).
//!
//! embassy-rp brings one in with its `binary-info` feature, but its
//! address map only covers `.data`, while here the entries' strings live in
//! `.ram_code` (memory.x), and so does the map itself. This header and map
//! stay in flash and cover both, so picotool can follow a run time SRAM
//! address back to where the bytes sit in the image.

use rp_binary_info::{EntryAddr, Header, MappingTableEntry};

unsafe extern "C" {
    static __bi_entries_start: EntryAddr;
    static __bi_entries_end: EntryAddr;
    static __ram_code_start: u32;
    static __ram_code_end: u32;
    static __ram_code_load: u32;
    static __sdata: u32;
    static __edata: u32;
    static __sidata: u32;
}

/// For each region copied to SRAM: where it is stored, where it runs.
#[unsafe(link_section = ".flash_rodata")]
static MAPPING: [MappingTableEntry; 3] = [
    MappingTableEntry {
        source_addr_start: &raw const __ram_code_load,
        dest_addr_start: &raw const __ram_code_start,
        dest_addr_end: &raw const __ram_code_end,
    },
    MappingTableEntry {
        source_addr_start: &raw const __sidata,
        dest_addr_start: &raw const __sdata,
        dest_addr_end: &raw const __edata,
    },
    MappingTableEntry::null(),
];

/// In the first 4 KB of flash (memory.x puts `.boot_info` there).
#[unsafe(link_section = ".boot_info")]
#[used]
static HEADER: Header = Header::new(&raw const __bi_entries_start, &raw const __bi_entries_end, &MAPPING);
