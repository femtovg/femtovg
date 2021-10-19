use std::io::Seek;

use byteorder::{BigEndian, ByteOrder, ReadBytesExt, WriteBytesExt};

// TODO: Remove this when drain_filter comes to stable rust

pub(crate) trait VecRetainMut<T> {
    fn retain_mut<F>(&mut self, f: F)
    where
        F: FnMut(&mut T) -> bool;
}

impl<T> VecRetainMut<T> for Vec<T> {
    // Adapted from libcollections/vec.rs in Rust
    // Primary author in Rust: Michael Darakananda
    fn retain_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        let len = self.len();
        let mut del = 0;
        {
            let v = &mut **self;

            for i in 0..len {
                if !f(&mut v[i]) {
                    del += 1;
                } else if del > 0 {
                    v.swap(i - del, i);
                }
            }
        }
        if del > 0 {
            self.truncate(len - del);
        }
    }
}

// Remove this when ttf-parser is updated from 0.12.3

/// Changes the usWin* ascender/ descender values to match the usTypo* ascender/ descender values in a font file in memory
/// 
/// Due to a bug in the latest version of ttf-parser (0.12.3), which is depended on by RustyBuzz, the wrong ascender/ descender
/// metrics are used for the entypo font file. This is a temporary fix until ttf-parser and RustyBuzz are updated.
pub(crate) fn sync_typographic_metrics(mut font_data: &mut [u8]) -> Result<(), std::io::Error> {

    let num_tables = (&font_data[4..6]).read_i16::<BigEndian>()?;
    // Find the OS/2 table in the font file
    let mut i = 12;
    for _ in 0..num_tables {
        let data = &font_data[i..i+16];
        let tag = &data[0..4];
        let checksum = (&data[4..8]).read_u32::<BigEndian>()?; 
        let offset = (&data[8..12]).read_u32::<BigEndian>()? as usize;
         
        let length = (&data[12..16]).read_u32::<BigEndian>()? as usize;
        i += 16;

        // Locate the OS/2 table
        if tag == &[0x4F, 0x53, 0x2F, 0x32] {
            let os2 = &font_data[offset..offset+length];
            let typo_ascender = (&os2[68..70]).read_i16::<BigEndian>()?;
            let typo_descender = (&os2[70..72]).read_i16::<BigEndian>()?;
            (&mut font_data[(offset + 74)..(offset + 76)]).write_i16::<BigEndian>(typo_ascender)?;
            (&mut font_data[(offset + 76)..(offset + 78)]).write_i16::<BigEndian>(-typo_descender)?;
            break;
        }

        
    }
    
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;


    static FONT_DATA: &[u8] = include_bytes!("../examples/assets/entypo.ttf");

    #[test]
    fn test_sync_typographic_metrics() {
        let mut data = FONT_DATA.to_owned();
        sync_typographic_metrics(&mut data);

        std::fs::write("test_file.ttf", &data);
    }
}