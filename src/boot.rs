
use packing::Packed;

use crate::Config;

/// Encodable Boot Block object
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[cfg_attr(feature="defmt", derive(defmt::Format))]
#[packed(little_endian, lsb0)]
pub struct FatBootBlock {
    #[pkd(7, 0, 0, 2)]
    pub jump_instruction: [u8; 3],

    #[pkd(7, 0, 3, 10)]
    pub oem_info: [u8; 8],
    
    #[pkd(7, 0, 11, 12)]
    pub bytes_per_sector: u16,
    
    #[pkd(7, 0, 13, 13)]
    pub sectors_per_cluster: u8,
    
    #[pkd(7, 0, 14, 15)]
    pub reserved_sectors: u16,
    
    #[pkd(7, 0, 16, 16)]
    pub fat_copies: u8,
    
    #[pkd(7, 0, 17, 18)]
    pub root_directory_entries: u16,
    
    #[pkd(7, 0, 19, 20)]
    pub total_sectors16: u16,
    
    #[pkd(7, 0, 21, 21)]
    pub media_descriptor: u8,
    
    #[pkd(7, 0, 22, 23)]
    pub sectors_per_fat: u16,
    
    #[pkd(7, 0, 24, 25)]
    pub sectors_per_track: u16,
    
    #[pkd(7, 0, 26, 27)]
    pub heads: u16,
    
    #[pkd(7, 0, 28, 31)]
    pub hidden_sectors: u32,
    
    #[pkd(7, 0, 32, 35)]
    pub total_sectors32: u32,
    
    #[pkd(7, 0, 36, 36)]
    pub physical_drive_num: u8,
    
    #[pkd(7, 0, 37, 37)]
    _reserved: u8,
    
    #[pkd(7, 0, 38, 38)]
    pub extended_boot_sig: u8,
    
    #[pkd(7, 0, 39, 42)]
    pub volume_serial_number: u32,
    
    #[pkd(7, 0, 43, 53)]
    pub volume_label: [u8; 11],
    
    #[pkd(7, 0, 54, 61)]
    pub filesystem_identifier: [u8; 8],
}

impl FatBootBlock {

    /// Create a new FAT BootBlock with the provided config
    pub fn new<const BLOCK_SIZE: usize>(config: &Config<BLOCK_SIZE>) -> FatBootBlock {

        let mut fat = FatBootBlock {
            jump_instruction: [0xEB, 0x3C, 0x90],
            oem_info: [0x20; 8],
            bytes_per_sector: BLOCK_SIZE as u16,
            sectors_per_cluster: 1,
            reserved_sectors: config.reserved_sectors as u16,
            fat_copies: 2,
            root_directory_entries: (config.root_dir_sectors as u16 * 512 / 32),
            total_sectors16: config.num_blocks as u16 - 2,
            media_descriptor: 0xF8,
            sectors_per_fat: config.sectors_per_fat() as u16,
            sectors_per_track: 1,
            heads: 1,
            hidden_sectors: 0,
            total_sectors32: 0,
            physical_drive_num: 0,
            _reserved: 0,
            extended_boot_sig: 0x29,
            volume_serial_number: 0x00420042,
            volume_label: [0x20; 11],
            filesystem_identifier: [0x20; 8],
        };

        let len = usize::min(fat.oem_info.len() - 1, config.oem_info.as_bytes().len());
        fat.oem_info[..len].copy_from_slice(&config.oem_info.as_bytes()[..len]);

        let len = usize::min(fat.volume_label.len() - 1, config.volume_label.as_bytes().len());
        fat.volume_label[..len].copy_from_slice(&config.volume_label.as_bytes()[..len]);

        let len = usize::min(fat.filesystem_identifier.len() - 1, config.filesystem_identifier.as_bytes().len());
        fat.filesystem_identifier[..len].copy_from_slice(&config.filesystem_identifier.as_bytes()[..len]);

        crate::debug!("BootBlock: {:?}", fat);

        fat
    }
}
