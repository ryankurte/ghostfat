//! GhostFAT Virtual FAT implementation for embedded USB SCSI devices
//! 
// Based on: https://github.com/cs2dsb/stm32-usb.rs/blob/master/firmware/usb_bootloader/src/ghost_fat.rs

#![cfg_attr(not(feature="std"), no_std)]
#![cfg_attr(feature="nightly", feature(const_mut_refs))]

#[cfg(feature = "defmt")]
use defmt::{debug, info, trace, warn, error};

#[cfg(not(feature = "defmt"))]
use log::{debug, info, trace, warn, error};

use packing::{Packed, PackedSize};

use usbd_scsi::{BlockDevice, BlockDeviceError};

mod config;
pub use config::Config;

mod file;
pub use file::{File, FileContent, DynamicFile};

mod boot;
use boot::FatBootBlock;

mod dir;
use dir::DirectoryEntry;

const ASCII_SPACE: u8 = 0x20;


/// Virtual FAT16 File System
pub struct GhostFat<'a, const BLOCK_SIZE: usize = 512> {
    config: Config<BLOCK_SIZE>,
    fat_boot_block: FatBootBlock,
    pub(crate) fat_files: &'a mut [File<'a, BLOCK_SIZE>],
}

impl <'a, const BLOCK_SIZE: usize> GhostFat<'a, BLOCK_SIZE> {
    /// Create a new file system instance with the provided files and configuration
    pub fn new(files: &'a mut [File<'a, BLOCK_SIZE>], config: Config<BLOCK_SIZE>) -> Self {

        debug!("Configuring ghostfat with {} {} byte sectors ({} byte total), {} sector FATs", config.num_blocks, BLOCK_SIZE, config.num_blocks as usize * BLOCK_SIZE, config.sectors_per_fat());

        Self {
            fat_boot_block: FatBootBlock::new(&config),
            fat_files: files,
            config,
        }
    }

    fn fat(id: usize, files: &[File<BLOCK_SIZE>], block: &mut [u8]){
        let mut index = 0;

        // Clear block
        for b in block.iter_mut() {
            *b = 0;
        }

        // First FAT contains media and file end marker in clusters 0 and 1
        if id == 0 {
            block[0] = 0xf0;
            block[1] = 0xff;
            block[2] = 0xff;
            block[3] = 0xff;
            index = 2;
        }

        // Compute cluster offset from FAT ID
        let cluster_offset = id * BLOCK_SIZE / 2;
        // Allocated blocks start at two to avoid reserved sectors
        let mut block_index = 2;

        // Iterate through available files to allocate blocks
        for f in files.iter() {
            // Determine number of blocks required for each file
            let block_count = f.num_blocks();

            // Skip entries where file does not overlap FAT
            //#[cfg(nope)]
            if (block_index + block_count < cluster_offset) || (block_index > cluster_offset + BLOCK_SIZE/1) {
                block_index += block_count;
                continue;
            }

            if cluster_offset >= block_index + block_count {
                block_index += block_count;
                continue;
            }
            
            println!("FAT {} File: '{}' {} clusters starting at cluster {}", id, f.name(), block_count, block_index);

            let (file_offset, remainder) = if cluster_offset > block_index {
                (cluster_offset - block_index, block_count + block_index - cluster_offset)
            } else {
                (0, block_count)
            };

            let blocks = usize::min(remainder, (BLOCK_SIZE / 2) - (index % BLOCK_SIZE));

            println!("FAT offset: {} file offset: {} remainder: {} clusters: {}", cluster_offset, file_offset, remainder, blocks);

            for i in 0..blocks {
                let j = i * 2;

                let v: u16 = if remainder == blocks && i == blocks-1 {
                    0xFFFF
                } else {
                    (block_index + file_offset + i + 1) as u16
                };

                block[index * 2 + j] =  v as u8;
                block[index * 2 + j + 1] = (v >> 8) as u8;
            }

            // Increase FAT index
            index += blocks;

            // Increase block index
            block_index += blocks;
        }
    } 

}

/// [`BlockDevice`] implementation for use with [`usbd_scsi`]
impl <'a, const BLOCK_SIZE: usize>BlockDevice for GhostFat<'a, BLOCK_SIZE> {
    const BLOCK_BYTES: usize = BLOCK_SIZE;

    /// Read a file system block
    fn read_block(&self, lba: u32, block: &mut [u8]) -> Result<(), BlockDeviceError> {
        assert_eq!(block.len(), Self::BLOCK_BYTES);

        trace!("GhostFAT reading lba: {} ({} bytes)", lba, block.len());

        // Clear the buffer since we're sending all of it
        for b in block.iter_mut() {
            *b = 0
        }

        // Block 0 is the fat boot block
        if lba == 0 {
            self.fat_boot_block
                .pack(&mut block[..FatBootBlock::BYTES])
                .unwrap();
            block[510] = 0x55;
            block[511] = 0xAA;

        // File allocation table(s) follow the boot block
        } else if lba < self.config.start_rootdir() {
            let mut section_index = lba - self.config.start_fat0();

            debug!("Read FAT section index: {} (lba: {})", section_index, lba);

            // The file system contains two copies of the FAT
            // wrap the section index to overlap these
            if section_index >= self.config.sectors_per_fat() {
                section_index -= self.config.sectors_per_fat();
            }

            Self::fat(section_index as usize, &self.fat_files, block);
            trace!("FAT {}: {:?}", section_index, &block);

        // Directory entries follow
        } else if lba < self.config.start_clusters() {
            let section_index = lba - self.config.start_rootdir();
            if section_index == 0 {
                let mut dir = DirectoryEntry::default();
                dir.name.copy_from_slice(&self.fat_boot_block.volume_label);
                dir.attrs = 0x28;

                let len = DirectoryEntry::BYTES;
                dir.pack(&mut block[..len]).unwrap();
                dir.attrs = 0;

                // Starting cluster index (after BBL and FAT)
                let mut cluster_index = 2;

                // Generate directory entries for registered files
                for (i, info) in self.fat_files.iter().enumerate() {
                    // Determine number of blocks required for each file
                    let mut block_count = info.len() / Self::BLOCK_BYTES;
                    if info.len() % Self::BLOCK_BYTES != 0 {
                        block_count += 1;
                    }
                    dir.start_cluster = cluster_index as u16;

                    // Write attributes
                    dir.name.copy_from_slice(&info.short_name().unwrap());
                    dir.size = info.len() as u32;
                    dir.attrs = info.attrs().bits();

                    // Encode to block
                    let start = (i + 1) * len;
                    dir.pack(&mut block[start..(start + len)]).unwrap();

                    // Increment cluster index
                    cluster_index += block_count;
                }
            }

        // Then finally clusters (containing actual data)
        } else {
            let section_index = (lba - self.config.start_clusters()) as usize;

            debug!("Read cluster index: 0x{:04x} (lba: 0x{:04x})", section_index, lba);

            // Iterate through files to find matching block
            let mut block_index = 0;
            for f in self.fat_files.iter() {

                // Determine number of blocks required for each file
                let mut block_count = f.len() / Self::BLOCK_BYTES;
                if f.len() % Self::BLOCK_BYTES != 0 {
                    block_count += 1;
                }

                // If the LBA is within the file, return data
                if section_index < block_count + block_index {
                    let offset = section_index - block_index;

                    debug!("Read file: {} chunk: 0x{:02x}", f.name(), offset);

                    if f.chunk(offset, block) == 0 {
                        warn!("Failed to read file: {} chunk: {}", f.name(), offset);
                    }

                    return Ok(())
                }

                // Otherwise, continue
                block_index += block_count;
            }

            warn!("Unhandled cluster read 0x{:04x} (lba: 0x{:04x})", section_index, lba);
        }
        Ok(())
    }

    /// Write a file system block
    fn write_block(&mut self, lba: u32, block: &[u8]) -> Result<(), BlockDeviceError> {
        debug!("GhostFAT writing lba: {} ({} bytes)", lba, block.len());

        if lba == 0 {
            warn!("Attempted write to boot sector");
            return Ok(());

        // Write to FAT
        } else if lba < self.config.start_rootdir() {
            // TODO: should we support this?
            warn!("Attempted to write to FAT");

        // Write directory entry
        } else if lba < self.config.start_clusters() {
            // TODO: do we need to wrap this somehow to remap writes?
            // it _appears_ it's okay to assume the FAT driver will use existing
            // allocated blocks so this is not required provided files do not exceed
            // configured sizes
            warn!("Attempted to write directory entries");

            let section_index = lba - self.config.start_rootdir();
            if section_index == 0 {


            }

        // Write cluster data
        } else {
            let section_index = (lba - self.config.start_clusters()) as usize;

            // Iterate through files to find matching block
            let mut block_index = 0;
            for f in self.fat_files.iter_mut() {

                // Determine number of blocks required for each file
                let mut block_count = f.len() / Self::BLOCK_BYTES;
                if f.len() % Self::BLOCK_BYTES != 0 {
                    block_count += 1;
                }

                // If the LBA is within the file, write data
                if section_index < block_count + block_index {
                    let offset = section_index - block_index;

                    debug!("Write file: {} block: {}, {} bytes", f.name(), offset, block.len());

                    if f.chunk_mut(offset, &block) == 0 {
                        error!("Attempted to write to read-only file");
                        return Err(BlockDeviceError::WriteError);
                    }

                    return Ok(())
                }

                // Otherwise, continue
                block_index += block_count;
            }

            debug!("Unhandled write section: {}", section_index);
        }

        Ok(())
    }

    /// Report the maximum block index for the file system
    fn max_lba(&self) -> u32 {
        self.config.num_blocks - 1
    }
}

#[cfg(test)]
mod tests {
    use crate::{GhostFat, File};


    #[test]
    fn file_offsets() {
        let data = [0xAAu8; 64];
        let f = [File::<8>::new_ro("test.bin", &data)];
        assert_eq!(f[0].len(), data.len());

        let mut block = [0u8; 8];
        GhostFat::fat(0, &f, &mut block);
        println!("FAT0: {:02x?}", block);

        assert_eq!(&block, &[
            0xf0, 0xff, 0xff, 0xff, 
            0x03, 0x00, 0x04, 0x00]);


        GhostFat::fat(1, &f, &mut block);
        println!("FAT1: {:02x?}", block);
        assert_eq!(&block, &[
            0x05, 0x00, 0x06, 0x00, 
            0x07, 0x00, 0x08, 0x00]);

        GhostFat::fat(2, &f, &mut block);
        println!("FAT2: {:02x?}", block);
        assert_eq!(&block, &[
            0x09, 0x00, 0xff, 0xff, 
            0x00, 0x00, 0x00, 0x00]);

        assert!(true);
    }

}
