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
        Self {
            fat_boot_block: FatBootBlock::new(&config),
            fat_files: files,
            config,
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

            debug!("Read FAT section index: {}", section_index);

            // TODO: why?
            // https://github.com/lupyuen/bluepill-bootloader/blob/master/src/ghostfat.c#L207
            if section_index >= self.config.sectors_per_fat() {
                section_index -= self.config.sectors_per_fat();
            }

            // Track allocated block count
            let mut index = 2;
            block[0] = 0xf0;

            // Set allocations for static files
            if section_index == 0 || true {

                // Allocate blocks for each file
                for f in self.fat_files.iter() {
                    // Determine number of blocks required for each file
                    let mut block_count = f.len() / Self::BLOCK_BYTES;
                    if f.len() % Self::BLOCK_BYTES != 0 {
                        block_count += 1;
                    }

                    trace!("File: {}, {} blocks starting at {}", f.name(), block_count, index);

                    // Write block allocations (2 byte)
                    for i in 0..block_count {
                        let j = i * 2;

                        if i == block_count - 1 {
                            // Final block contains 0xFFFF
                            block[index * 2 + j] = 0xFF;
                            block[index * 2 + j + 1] = 0xFF;
                        } else {
                            // Preceding blocks should link to next object
                            // TODO: not sure this linking is correct... should split and test
                            block[index * 2 + j] =  (index + i + 1) as u8;
                            block[index * 2 + j + 1] = ((index + i + 1) >> 8) as u8;
                        }
                    }

                    // Increase block index
                    index += block_count;
                }

                // Add trailer
                for i in 0..4 {
                    block[index * 2 + i] = 0xFF;
                }
                index += 4;
                let _ = index;
            }

            // Lock further chunks
            for b in &mut block[index*2..] {
                *b = 0xFE;
            }

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
                    }

                    return Ok(())
                }

                // Otherwise, continue
                block_index += block_count;
            }

            warn!("Unhandled write section: {}", section_index);
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
    use std::io::{Read, Seek, Write, SeekFrom};
    use std::sync::{Arc, Mutex};
    use log::{trace, debug, info};

    use simplelog::{SimpleLogger, LevelFilter, Config as LogConfig};

    use fatfs::{FsOptions, FatType};
    use usbd_scsi::BlockDevice;

    use crate::{GhostFat, File, config::Config};

    pub struct MockDisk<'a> {
        pub index: usize,
        pub disk: GhostFat<'a>,
    }

    // TODO: read/write do not yet handle multiple blocks

    impl <'a> Read for MockDisk<'a> {
        fn read(&mut self, buff: &mut [u8]) -> std::io::Result<usize> {
            // Map block to index and buff len
            let mut lba = self.index as u32 / 512;
            let offset = self.index as usize % 512;

            let mut block = [0u8; 512];
            let mut index = 0;

            // If we're offset and reading > 1 block, handle partial block first
            if offset > 0 && buff.len() > (512 - offset) {
                trace!("Read offset chunk lba: {} offset: {} len: {}", lba, offset, 512-offset);

                // Read entire block
                self.disk.read_block(lba, &mut block).unwrap();

                // Copy offset portion
                buff[..512 - offset].copy_from_slice(&block[offset..]);

                // Update indexes
                index += 512 - offset;
                lba += 1;
            }

            // Then read remaining aligned blocks
            for c in (&mut buff[index..]).chunks_mut(512) {
                // Read whole block
                self.disk.read_block(lba, &mut block).unwrap();

                // Copy back requested chunk
                // Note offset can only be < BLOCK_SIZE when there's only one chunk
                c.copy_from_slice(&block[offset..][..c.len()]);

                // Update indexes
                index += c.len();
                lba += 1;
            }
            
            trace!("Read {} bytes at index 0x{:02x} (lba: {} offset: 0x{:02x}), data: {:02x?}", buff.len(), self.index, lba, offset, buff);

            // Increment index
            self.index += buff.len();

            Ok(buff.len())
        }
    }

    impl <'a> Write for MockDisk<'a> {
        fn write(&mut self, buff: &[u8]) -> std::io::Result<usize> {

            // Map block to index and buff len
            let lba = self.index as u32 / 512;
            let offset = self.index as usize % 512;

            trace!("Write {} bytes at index: 0x{:02x} (lba: {} offset: 0x{:02x}): data: {:02x?}", buff.len(), self.index, lba, offset, buff);


            {
                // Read whole block
                let mut block = [0u8; 512];
                self.disk.read_block(lba, &mut block).unwrap();

                // Apply write to block
                block[offset..][..buff.len()].copy_from_slice(buff);

                // Write whole block
                self.disk.write_block(lba, &block).unwrap();
            }

            #[cfg(nope)]
            // Direct write to provide more information in tests
            d.write(self.index as u32, buff).unwrap();

            // Increment index
            self.index += buff.len();

            Ok(buff.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            // No flush required as we're immediately writing back
            Ok(())
        }
    }

    impl <'a> Seek for MockDisk<'a> {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            // Handle seek mechanisms
            match pos {
                SeekFrom::Start(v) => self.index = v as usize,
                SeekFrom::End(v) => {
                    todo!("Work out how long the disk is...");
                },
                SeekFrom::Current(v) => self.index = (self.index as i64 + v) as usize,
            }

            Ok(self.index as u64)
        }
    }

    fn setup<'a>(files: &'a mut [File<'a>]) -> MockDisk<'a> {
        let _ = simplelog::TermLogger::init(LevelFilter::Info, LogConfig::default(), simplelog::TerminalMode::Mixed, simplelog::ColorChoice::Auto);

        let ghost_fat = GhostFat::new(files, Config::default());

        // Setup mock disk for fatfs
        let disk = MockDisk{
            index: 0,
            disk: ghost_fat,
        };

        disk
    }

    #[test]
    fn read_small_file() {

        // GhostFAT files
        let data = b"UF2 Bootloader 1.2.3\r\nModel: BluePill\r\nBoard-ID: xyz_123\r\n";
        let files = &mut [
            File::new("INFO_UF2.TXT", data).unwrap(),
        ];

        // Setup GhostFAT
        let disk = setup(files);

        // Setup fatfs
        let opts = FsOptions::new().update_accessed_date(false);
        let fs = fatfs::FileSystem::new(disk, opts).unwrap();
        assert_eq!(fs.fat_type(), FatType::Fat16);

        // Check base directory
        let root_dir = fs.root_dir();

        // Load files
        let f: Vec<_> = root_dir.iter().map(|v| v.unwrap() ).collect();
        log::info!("Files: {:?}", f);

        // Read first file
        assert_eq!(f[0].short_file_name(), "INFO_UF2.TXT");
        let mut f0 = f[0].to_file();
        
        let mut s0 = String::new();
        f0.read_to_string(&mut s0).unwrap();

        assert_eq!(s0.as_bytes(), data);
    }

    #[test]
    fn read_large_file() {

        let mut data = [0u8; 1024];
        for i in 0..data.len() {
            data[i] = rand::random::<u8>();
        }

        // GhostFAT files
        let files = &mut [
            File::new("TEST.BIN", &data).unwrap(),
        ];

        // Setup GhostFAT
        let disk = setup(files);

        // Setup fatfs
        let fs = fatfs::FileSystem::new(disk, FsOptions::new()).unwrap();
        assert_eq!(fs.fat_type(), FatType::Fat16);

        // Check base directory
        let root_dir = fs.root_dir();

        // Load files
        let f: Vec<_> = root_dir.iter().map(|v| v.unwrap() ).collect();
        log::info!("Files: {:?}", f);

        // Read first file
        assert_eq!(f[0].short_file_name(), "TEST.BIN");
        let mut f0 = f[0].to_file();
        
        let mut v0 = Vec::new();
        f0.read_to_end(&mut v0).unwrap();

        assert_eq!(v0.as_slice(), data);
    }

    #[test]
    fn write_small_file() {

        // GhostFAT files
        let mut data = [0u8; 8];
        let files = &mut [
            File::new("TEST.TXT", data.as_mut()).unwrap(),
        ];

        // Setup GhostFAT
        let disk = setup(files);

        // Setup fatfs
        let fs = fatfs::FileSystem::new(disk, FsOptions::new()).unwrap();
        assert_eq!(fs.fat_type(), FatType::Fat16);

        // Check base directory
        let root_dir = fs.root_dir();

        // Load files
        let f: Vec<_> = root_dir.iter().map(|v| v.unwrap() ).collect();
        log::info!("Files: {:?}", f);

        // Fetch first file
        assert_eq!(f[0].short_file_name(), "TEST.TXT");
        

        let d1 = b"DEF456\r\n";

        log::info!("Write file");

        // Rewind and write data
        let mut f0 = f[0].to_file();
        f0.write_all(d1).unwrap();
        f0.flush();
        drop(f0);

        log::info!("Read file");

        // Read back written data
        let mut f1 = f[0].to_file();
        let mut s0 = String::new();
        f1.read_to_string(&mut s0).unwrap();
        assert_eq!(s0.as_bytes(), d1);
    }

    #[test]
    fn write_large_file() {

        // GhostFAT files
        let mut data = [0u8; 1024];
        for i in 0..data.len() {
            data[i] = rand::random::<u8>();
        }

        let files = &mut [
            File::new("TEST.BIN", &mut data).unwrap(),
        ];

        // Setup GhostFAT
        let disk = setup(files);

        // Setup fatfs
        let fs = fatfs::FileSystem::new(disk, FsOptions::new()).unwrap();
        assert_eq!(fs.fat_type(), FatType::Fat16);

        // Check base directory
        let root_dir = fs.root_dir();

        // Load files
        let f: Vec<_> = root_dir.iter().map(|v| v.unwrap() ).collect();
        log::info!("Files: {:?}", f);

        // Fetch first file
        assert_eq!(f[0].short_file_name(), "TEST.BIN");

        let mut d1 = [0u8; 1024];
        for i in 0..d1.len() {
            d1[i] = rand::random::<u8>();
        }

        // Rewind and write data
        let mut f0 = f[0].to_file();
        f0.rewind();
        f0.write_all(&d1).unwrap();
        f0.flush();
        drop(f0);

        // Read back written data
        let mut f1 = f[0].to_file();
        let mut v0 = Vec::new();
        f1.read_to_end(&mut v0).unwrap();
        assert_eq!(v0.as_slice(), d1);
    }

    #[test]
    fn write_huge_file() {

        // GhostFAT files
        let mut data = [0u8; 64 * 1024];
        for i in 0..data.len() {
            data[i] = rand::random::<u8>();
        }

        let files = &mut [
            File::new("TEST.BIN", &mut data).unwrap(),
        ];

        // Setup GhostFAT
        let disk = setup(files);

        // Setup fatfs
        let fs = fatfs::FileSystem::new(disk, FsOptions::new()).unwrap();
        assert_eq!(fs.fat_type(), FatType::Fat16);

        // Check base directory
        let root_dir = fs.root_dir();

        // Load files
        let f: Vec<_> = root_dir.iter().map(|v| v.unwrap() ).collect();
        log::info!("Files: {:?}", f);

        // Fetch first file
        assert_eq!(f[0].short_file_name(), "TEST.BIN");

        let mut d1 = [0u8; 64 * 1024];
        for i in 0..d1.len() {
            d1[i] = rand::random::<u8>();
        }

        // Rewind and write data
        let mut f0 = f[0].to_file();
        f0.rewind();
        f0.write_all(&d1).unwrap();
        f0.flush();
        drop(f0);

        // Read back written data
        let mut f1 = f[0].to_file();
        let mut v0 = Vec::new();
        f1.read_to_end(&mut v0).unwrap();
        assert_eq!(v0.as_slice(), d1);
    }

    #[test]
    fn read_many_files() {

        // GhostFAT files
        let d1 = b"abc123456";
        let d2 = b"abc123457";
        
        let files = &mut [
            File::new("TEST1.TXT", d1).unwrap(),
            File::new("TEST2.TXT", d2).unwrap(),
        ];

        // Setup GhostFAT
        let disk = setup(files);

        // Setup fatfs
        let fs = fatfs::FileSystem::new(disk, FsOptions::new()).unwrap();
        assert_eq!(fs.fat_type(), FatType::Fat16);

        // Check base directory
        let root_dir = fs.root_dir();

        // Load files
        let f: Vec<_> = root_dir.iter().map(|v| v.unwrap() ).collect();
        log::info!("Files: {:?}", f);

        // Fetch first file
        assert_eq!(f[0].short_file_name(), "TEST1.TXT");
        
        // Read data
        let mut f1 = f[0].to_file();
        let mut s0 = String::new();
        f1.read_to_string(&mut s0).unwrap();
        assert_eq!(s0.as_bytes(), d1);

        // Fetch second file
        assert_eq!(f[1].short_file_name(), "TEST2.TXT");

        // Read data
        let mut f1 = f[1].to_file();
        let mut s0 = String::new();
        f1.read_to_string(&mut s0).unwrap();
        assert_eq!(s0.as_bytes(), d2);
    }
}
