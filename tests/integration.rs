use std::io::{Read, Seek, Write, SeekFrom};
use std::sync::{Arc, Mutex};
use log::{trace, debug, info};

use simplelog::{SimpleLogger, LevelFilter, Config as LogConfig};

use fatfs::{FsOptions, FatType};
use usbd_scsi::BlockDevice;

use ghostfat::{GhostFat, File, Config};

/// Mock disk implementation for fatfs support
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

fn read_file<const N: usize>() {
    // Setup data
    let mut data = [0u8; N];
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

    assert_eq!(v0.as_slice() == data, true);
}

#[test]
fn read_small_file() {
    read_file::<64>();
}

#[test]
fn read_multi_cluster_file() {
    read_file::<1024>();
}

#[test]
#[ignore = "not yet supported"]
fn read_multi_fat_file() {
    read_file::<200_000>();
}

fn write_file<const N: usize>() {

    // Generate initial data
    let mut data = [0u8; N];
    for i in 0..data.len() {
        data[i] = rand::random::<u8>();
    }

    // Setup GhostFAT
    let files = &mut [
        File::new("TEST.BIN", &mut data).unwrap(),
    ];
    let disk = setup(files);

    // Setup fatfs
    let fs = fatfs::FileSystem::new(disk, FsOptions::new()).unwrap();
    assert_eq!(fs.fat_type(), FatType::Fat16);

    // Check base directory
    let root_dir = fs.root_dir();

    // Load files
    let f: Vec<_> = root_dir.iter().map(|v| v.unwrap() ).collect();
    log::info!("Files: {:?}", f);

    // Fetch file handle
    assert_eq!(f[0].short_file_name(), "TEST.BIN");

    // Generate new data
    let mut d1 = [0u8; N];
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
fn write_small_file() {
    write_file::<64>();
}

#[test]
fn write_multi_cluster_file() {
    write_file::<64_000>();
}

#[test]
#[ignore = "not yet supported"]
fn write_multi_fat_file() {
    write_file::<128_000>();
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
