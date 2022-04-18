

/// Virtual file system configuration
pub struct Config<const BLOCK_SIZE: usize = 512> {
    /// Number of blocks in the file system
    pub num_blocks: u32,
    /// Reserved sectors
    pub reserved_sectors: u32,
    /// Root directory sectors
    pub root_dir_sectors: u32,

    /// OEM info, defaults to "UF2 UF2"
    pub oem_info: &'static str,

    /// Volume label, defaults to "GHOSTFAT"
    pub volume_label: &'static str,

    /// FileSystem Identifier, defaults to "FAT16"
    pub filesystem_identifier: &'static str,

    /// Force use of Default::default() for construction
    _reserved: (),
}

impl <const BLOCK_SIZE: usize> Default for Config<BLOCK_SIZE> {
    fn default() -> Self {
        Self { 
            num_blocks: 8000,
            reserved_sectors: 1,
            root_dir_sectors: 4,
            oem_info: "UF2 UF2",
            volume_label: "GHOSTFAT",
            filesystem_identifier: "FAT16",
            _reserved: (),
        }
    }
}

impl <const BLOCK_SIZE: usize> Config<BLOCK_SIZE> {

    /// Fetch the block/sector size
    pub const fn sector_size(&self) -> u32 {
        BLOCK_SIZE as u32
    }

    /// Calculate number of sectors per FAT
    pub const fn sectors_per_fat(&self) -> u32 {
        (self.num_blocks * 2 + BLOCK_SIZE as u32 - 1) / BLOCK_SIZE as u32
    }

    /// Calculate FAT0 start
    pub const fn start_fat0(&self) -> u32 {
        self.reserved_sectors
    }

    /// Calculate FAT1 start
    pub const fn start_fat1(&self) -> u32 {
        self.start_fat0() + self.sectors_per_fat()
    }

    /// Calculate ROOTDIR start
    pub const fn start_rootdir(&self) -> u32 {
        self.start_fat1() + self.sectors_per_fat()
    }

    /// Calculate cluster start
    pub const fn start_clusters(&self) -> u32 {
        self.start_rootdir() + self.root_dir_sectors
    }

    /// Encode config to boot block
    /// 
    /// See: [https://academy.cba.mit.edu/classes/networking_communications/SD/FAT.pdf]()
    pub fn encode(&self, block: &mut [u8]) {
        let mut index = 0;

        // Jump instruction
        block[index..][..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
        index += 3;
        

        // OEM info
        let len = usize::min(self.oem_info.len(), 8);
        block[index..][..len].copy_from_slice(&self.oem_info.as_bytes()[..len]);
        index += 8;


        todo!();
    }

}
