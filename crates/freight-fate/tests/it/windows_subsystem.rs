//! The packaged game must never open a terminal window.
//!
//! Whether it does is decided by one 16-bit field in the executable's PE
//! header, written by the linker from `#![windows_subsystem = "windows"]` in
//! `main.rs`. There is no way to observe it from inside a running process --
//! a build that lost the attribute looks perfectly healthy to every other
//! test in the suite, and the first person to notice is a tester who launches
//! the game and gets a black window beside it. So the assertion is made
//! against the built binary itself, which is the thing that regressed.

#![cfg(windows)]

use std::fs;
use std::process::Command;

/// `IMAGE_SUBSYSTEM_WINDOWS_GUI`: no console is created for the process.
const IMAGE_SUBSYSTEM_WINDOWS_GUI: u16 = 2;
/// `IMAGE_SUBSYSTEM_WINDOWS_CUI`: Windows gives the process a console window.
const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;

/// The `Subsystem` field of a PE image's optional header.
///
/// `e_lfanew` at offset 0x3C points at the `PE\0\0` signature; the COFF file
/// header is the next 20 bytes, and the optional header follows. `Subsystem`
/// sits 68 bytes into that optional header in both PE32 and PE32+ -- the
/// extra eight bytes PE32+ spends on `ImageBase` are cancelled by the
/// `BaseOfData` field it does not have.
fn pe_subsystem(bytes: &[u8]) -> u16 {
    let read_u16 = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let read_u32 = |at: usize| {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
    };
    assert_eq!(&bytes[..2], b"MZ", "not a DOS/PE image");
    let pe = read_u32(0x3C);
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0", "no PE signature at e_lfanew");
    let optional = pe + 4 + 20;
    let magic = read_u16(optional);
    assert!(
        magic == 0x10B || magic == 0x20B,
        "unexpected optional-header magic {magic:#x}"
    );
    read_u16(optional + 68)
}

/// The game binary is a GUI-subsystem image, so launching it opens no console.
#[test]
fn game_binary_is_linked_into_the_windows_gui_subsystem() {
    let exe = env!("CARGO_BIN_EXE_freightfate");
    let bytes = fs::read(exe).unwrap_or_else(|e| panic!("cannot read {exe}: {e}"));
    let subsystem = pe_subsystem(&bytes);
    assert_ne!(
        subsystem, IMAGE_SUBSYSTEM_WINDOWS_CUI,
        "{exe} is a console-subsystem image: launching the packaged game would \
         open a terminal window beside it. Restore `#![windows_subsystem = \
         \"windows\"]` at the top of crates/freight-fate/src/main.rs."
    );
    assert_eq!(
        subsystem, IMAGE_SUBSYSTEM_WINDOWS_GUI,
        "unexpected PE subsystem {subsystem} for {exe}"
    );
}

/// ...and the drive tools still print, which is what the console attach in
/// `main` buys back. A capturing parent (this test, `subprocess.run` in
/// `cargo tools build-release`) reads the child's pipe, so this covers the
/// piped/redirected half; the terminal half is the attach itself.
#[test]
fn help_still_prints_from_the_gui_subsystem_binary() {
    let exe = env!("CARGO_BIN_EXE_freightfate");
    let out = Command::new(exe)
        .arg("--help")
        .output()
        .unwrap_or_else(|e| panic!("cannot run {exe} --help: {e}"));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "--help exited {:?}",
        out.status.code()
    );
    assert!(
        text.contains("freightfate -- Freight Fate"),
        "--help printed nothing usable: {text:?}"
    );
}
