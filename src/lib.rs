// https://github.com/cs2dsb/stm32-usb.rs/blob/master/firmware/usb_bootloader/src/ghost_fat.rs

use core::ptr::read_volatile;

#[cfg(feature = "defmt")]
use defmt::{debug, info, trace, warn};

#[cfg(not(feature = "defmt"))]
use log::{debug, info, trace, warn};

use packing::{Packed, PackedSize};

use usbd_scsi::{BlockDevice, BlockDeviceError};

pub mod config;
pub use config::Config;

pub mod boot;
use boot::FatBootBlock;

pub mod dir;
use dir::DirectoryEntry;

pub mod file;
use file::{FatFile, FatFileContent};

const UF2_SIZE: u32 = 0x10000 * 2;
const UF2_SECTORS: u32 = UF2_SIZE / (512 as u32);

const ASCII_SPACE: u8 = 0x20;

pub fn fat_files() -> [FatFile; 3] {
    let info = FatFile::with_content(
        "INFO_UF2TXT",
        "UF2 Bootloader 1.2.3\r\nModel: BluePill\r\nBoard-ID: xyz_123\r\n",
    );
    let index = FatFile::with_content("INDEX   HTM", "<!doctype html>\n<html><body><script>\nlocation.replace(INDEX_URL);\n</script></body></html>\n");

    let mut name = [ASCII_SPACE; 11];
    name.copy_from_slice("CURRENT UF2".as_bytes());

    let current_uf2 = FatFile {
        name,
        content: FatFileContent::Uf2,
    };

    [info, index, current_uf2]
}

/// # Dummy fat implementation that provides a [UF2 bootloader](https://github.com/microsoft/uf2)
pub struct GhostFat {
    config: Config,
    fat_boot_block: FatBootBlock,
    fat_files: [FatFile; 3],
}

impl GhostFat {
    pub fn new(config: Config) -> Self {
        Self {
            fat_boot_block: FatBootBlock::new(&config),
            fat_files: fat_files(),
            config,
        }
    }
}

impl BlockDevice for GhostFat {
    const BLOCK_BYTES: usize = 512;

    fn read_block(&self, lba: u32, block: &mut [u8]) -> Result<(), BlockDeviceError> {
        assert_eq!(block.len(), Self::BLOCK_BYTES);

        debug!("GhostFAT reading block: {}", lba);

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

            // TODO: why?
            // https://github.com/lupyuen/bluepill-bootloader/blob/master/src/ghostfat.c#L207
            if section_index >= self.config.sectors_per_fat() {
                section_index -= self.config.sectors_per_fat();
            }

            // Set allocations for static files
            if section_index == 0 {
                block[0] = 0xF0;
                for i in 1..(self.fat_files.len() * 2 + 4) {
                    block[i] = 0xFF;
                }
            }

            // Assuming each file is one block, uf2 is offset by this
            let uf2_first_sector = self.fat_files.len() + 1;
            let uf2_last_sector = uf2_first_sector + UF2_SECTORS as usize - 1;

            // TODO: is this setting allocations for the uf2 file?
            for i in 0..256_usize {
                let v = section_index as usize * 256 + i;
                let j = 2 * i;
                if v >= uf2_first_sector && v < uf2_last_sector {
                    block[j + 0] = (((v + 1) >> 0) & 0xFF) as u8;
                    block[j + 1] = (((v + 1) >> 8) & 0xFF) as u8;
                } else if v == uf2_last_sector {
                    block[j + 0] = 0xFF;
                    block[j + 1] = 0xFF;
                }
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

                // Generate directory entries for registered files
                for (i, info) in self.fat_files.iter().enumerate() {
                    dir.name.copy_from_slice(&info.name);
                    dir.start_cluster = i as u16 + 2;
                    dir.size = match info.content {
                        FatFileContent::Static(content) => content.len() as u32,
                        FatFileContent::Uf2 => {
                            // TODO: set data length for this object
                            0
                        }
                    };
                    let start = (i + 1) * len;
                    dir.pack(&mut block[start..(start + len)]).unwrap();
                }
            }

        // Then finally clusters (containing actual data)
        } else {
            let section_index = (lba - self.config.start_clusters()) as usize;

            if section_index < self.fat_files.len() {
                let info = &self.fat_files[section_index];
                if let FatFileContent::Static(content) = &info.content {
                    block[..content.len()].copy_from_slice(content);
                }
            } else {
                //UF2
                debug!("Read UF2: {}", section_index);

                // TODO: read data and return
            }
        }
        Ok(())
    }

    fn write_block(&mut self, lba: u32, block: &[u8]) -> Result<(), BlockDeviceError> {
        debug!("GhostFAT writing block {}: {:?}", lba, block);

        if lba == 0 {
            warn!("Attempted write to boot sector");
            return Ok(());

        // Write to FAT
        } else if lba < self.config.start_rootdir() {

            // Write directory entry
        } else if lba < self.config.start_clusters() {

            // Write cluster data
        } else {
        }

        // TODO: write block to flash

        Ok(())
    }

    fn max_lba(&self) -> u32 {
        self.config.num_blocks - 1
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, Write, SeekFrom};
    use std::sync::{Arc, Mutex};
    use log::{debug};

    use simplelog::{SimpleLogger, LevelFilter, Config as LogConfig};

    use fatfs::{FsOptions, FatType};
    use usbd_scsi::BlockDevice;

    use crate::{GhostFat, config::Config};

    pub struct MockDisk {
        pub index: usize,
        pub disk: Arc<Mutex<GhostFat>>,
    }

    // TODO: read/write do not yet handle multiple blocks

    impl Read for MockDisk {
        fn read(&mut self, buff: &mut [u8]) -> std::io::Result<usize> {
            let d = self.disk.lock().unwrap();

            // Map block to index and buff len
            let lba = self.index as u32 / 512;
            let offset = self.index as usize % 512;

            debug!("Read {} bytes at index: 0x{:02x} (lba: {} offset: {})", buff.len(), self.index, lba, offset);

            // Read whole block
            let mut block = [0u8; 512];
            d.read_block(lba, &mut block).unwrap();

            // Copy back requested chunk
            buff.copy_from_slice(&block[offset..][..buff.len()]);
            
            debug!("Data: {:02x?}", buff);

            // Increment index
            self.index += buff.len();

            Ok(buff.len())
        }
    }

    impl Write for MockDisk {
        fn write(&mut self, buff: &[u8]) -> std::io::Result<usize> {
            let mut d = self.disk.lock().unwrap();


            // Map block to index and buff len
            let lba = self.index as u32 / 512;
            let offset = self.index as usize % 512;

            debug!("Write {} bytes at index: 0x{:02x} (lba: {} offset: {})", buff.len(), self.index, lba, offset);
            debug!("Data: {:02x?}", buff);

            // Read whole block
            let mut block = [0u8; 512];
            d.read_block(lba, &mut block).unwrap();

            // Apply write to block
            block[offset..][..buff.len()].copy_from_slice(buff);

            // Write whole block
            d.write_block(lba, &block).unwrap();

            // Increment index
            self.index += buff.len();

            Ok(buff.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            // No flush required as we're immediately writing back
            Ok(())
        }
    }

    impl Seek for MockDisk {
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

    #[test]
    fn it_works() {
        let _ = SimpleLogger::init(LevelFilter::Debug, LogConfig::default());

        // Setup mock disk for fatfs
        let disk = MockDisk{
            index: 0,
            disk: Arc::new(Mutex::new(GhostFat::new(Config::default()))),
        };

        // Setup file system
        let fs = fatfs::FileSystem::new(disk, FsOptions::new()).unwrap();

        // Check file system setup
        assert_eq!(fs.fat_type(), FatType::Fat16);

        // Check base directory
        let root_dir = fs.root_dir();

        let files: Vec<_> = root_dir.iter().map(|v| v.unwrap() ).collect();

        log::info!("Files: {:?}", files);

        assert_eq!(files[0].short_file_name(), "INFO_UF2.TXT");
        assert_eq!(files[1].short_file_name(), "INDEX.HTM");
    }
}
