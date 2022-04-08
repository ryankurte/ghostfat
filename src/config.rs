

pub struct Config<const BLOCK_SIZE: u32 = 512> {
    pub num_blocks: u32,
    pub reserved_sectors: u32,
    pub root_dir_sectors: u32,

}

impl <const BLOCK_SIZE: u32> Default for Config<BLOCK_SIZE> {
    fn default() -> Self {
        Self { 
            num_blocks: 8000,
            reserved_sectors: 1,
            root_dir_sectors: 4,
        }
    }
}

impl <const BLOCK_SIZE: u32> Config<BLOCK_SIZE> {

    pub  const fn sector_size(&self) -> u32 {
        BLOCK_SIZE
    }

    pub  const fn sectors_per_fat(&self) -> u32 {
        (self.num_blocks * 2 + BLOCK_SIZE - 1) / BLOCK_SIZE
    }

    pub  const fn start_fat0(&self) -> u32 {
        self.reserved_sectors
    }

    pub  const fn start_fat1(&self) -> u32 {
        self.start_fat0() + self.sectors_per_fat()
    }

    pub  const fn start_rootdir(&self) -> u32 {
        self.start_fat1() + self.sectors_per_fat()
    }

    pub  const fn start_clusters(&self) -> u32 {
        self.start_rootdir() + self.root_dir_sectors
    }
}