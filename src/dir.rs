use packing::Packed;

#[derive(Clone, Copy, Default, Packed)]
#[packed(little_endian, lsb0)]
pub struct DirectoryEntry {    
    #[pkd(7, 0, 0, 10)]
    pub name: [u8; 11],
    /*
        pub name: [u8; 8],
        pub ext: [u8; 3],
    */
    #[pkd(7, 0, 11, 11)]
    pub attrs: u8,

    #[pkd(7, 0, 12, 12)]
    _reserved: u8,

    #[pkd(7, 0, 13, 13)]
    pub create_time_fine: u8,

    #[pkd(7, 0, 14, 15)]
    pub create_time: u16,

    #[pkd(7, 0, 16, 17)]
    pub create_date: u16,
    
    #[pkd(7, 0, 18, 19)]
    pub last_access_date: u16,
    
    #[pkd(7, 0, 20, 21)]
    pub high_start_cluster: u16,
    
    #[pkd(7, 0, 22, 23)]
    pub update_time: u16,
    
    #[pkd(7, 0, 24, 25)]
    pub update_date: u16,
    
    #[pkd(7, 0, 26, 27)]
    pub start_cluster: u16,
    
    #[pkd(7, 0, 28, 31)]
    pub size: u32,
}
