//! Direct RGBA Kitty commands. Image *numbers* request terminal-allocated IDs;
//! subsequent commands refer to the newest image with that number. Never delete
//! the terminal's global image set. See the protocol links in README.md.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

pub const CHUNK: usize = 4096;
pub const MAX_IMAGES: usize = 9;
pub const MAX_PLACEMENTS: usize = 53;

#[derive(Default, Debug)]
pub struct Registry {
    images: BTreeSet<u32>,
    placements: BTreeSet<(u32, u32)>,
}

impl Registry {
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
    pub fn placement_count(&self) -> usize {
        self.placements.len()
    }
    pub fn upload(
        &mut self,
        out: &mut Vec<u8>,
        number: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> io::Result<()> {
        if number == 0
            || rgba.len() != width as usize * height as usize * 4
            || width == 0
            || height == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid RGBA sprite",
            ));
        }
        if self.images.contains(&number) {
            return Err(io::Error::other(
                "image number already uploaded; clean up before replacing",
            ));
        }
        if self.images.len() >= MAX_IMAGES {
            return Err(io::Error::other("sprite registry limit"));
        }
        // Register before output, so a partial transmission is also cleaned up.
        self.images.insert(number);
        let encoded = STANDARD.encode(rgba);
        let count = encoded.len().div_ceil(CHUNK);
        for (index, data) in encoded.as_bytes().chunks(CHUNK).enumerate() {
            let more = u8::from(index + 1 < count);
            if index == 0 {
                write!(
                    out,
                    "\x1b_Ga=t,t=d,f=32,I={number},s={width},v={height},q=2,m={more};"
                )?;
            } else {
                write!(out, "\x1b_Gm={more},q=2;")?;
            }
            out.extend_from_slice(data);
            out.extend_from_slice(b"\x1b\\");
        }
        Ok(())
    }
    pub fn place(
        &mut self,
        out: &mut Vec<u8>,
        number: u32,
        placement: u32,
        at: PixelPlacement,
        z: i32,
    ) -> io::Result<()> {
        self.place_clipped(out, number, placement, at, z, None)
    }
    pub fn place_clipped(
        &mut self,
        out: &mut Vec<u8>,
        number: u32,
        placement: u32,
        at: PixelPlacement,
        z: i32,
        height: Option<u32>,
    ) -> io::Result<()> {
        if !self.images.contains(&number) || placement == 0 {
            return Err(io::Error::other("unowned sprite placement"));
        }
        if !self.placements.contains(&(number, placement))
            && self.placements.len() >= MAX_PLACEMENTS
        {
            return Err(io::Error::other("placement registry limit"));
        }
        self.placements.insert((number, placement));
        write!(
            out,
            "\x1b[{};{}H\x1b_Ga=p,I={number},p={placement},X={},Y={},z={z},C=1,q=2",
            at.row + 1,
            at.column + 1,
            at.x_offset,
            at.y_offset
        )?;
        if let Some(h) = height {
            write!(out, ",h={h}")?;
        }
        out.extend_from_slice(b";\x1b\\");
        Ok(())
    }
    pub fn remove(&mut self, out: &mut Vec<u8>, number: u32, placement: u32) -> io::Result<()> {
        if self.placements.remove(&(number, placement)) {
            write!(out, "\x1b_Ga=d,d=n,I={number},p={placement},q=2;\x1b\\")?;
        }
        Ok(())
    }
    pub fn cleanup(&mut self, out: &mut impl Write) -> io::Result<()> {
        let mut first = None;
        for &number in &self.images {
            if let Err(error) = write!(out, "\x1b_Ga=d,d=N,I={number},q=2;\x1b\\") {
                first.get_or_insert(error);
            }
        }
        self.images.clear();
        self.placements.clear();
        first.map_or(Ok(()), Err)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelPlacement {
    pub column: u32,
    pub row: u32,
    pub x_offset: u32,
    pub y_offset: u32,
}

/// Flush once per frame. Drop sends the closing marker even if writing the body
/// fails or unwinds; no synchronized-update mode is left latched.
pub fn frame(out: &mut impl Write, body: &[u8], synchronized: bool) -> io::Result<()> {
    struct End<'a, W: Write> {
        out: &'a mut W,
        active: bool,
    }
    impl<W: Write> Drop for End<'_, W> {
        fn drop(&mut self) {
            if self.active {
                let _ = self.out.write_all(b"\x1b[?2026l");
                let _ = self.out.flush();
            }
        }
    }
    let mut guard = End {
        out,
        active: synchronized,
    };
    if synchronized {
        guard.out.write_all(b"\x1b[?2026h")?;
    }
    guard.out.write_all(body)?;
    if synchronized {
        guard.out.write_all(b"\x1b[?2026l")?;
        guard.active = false;
    }
    guard.out.flush()
}
