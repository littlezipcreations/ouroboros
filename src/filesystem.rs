use crate::virtio::VirtioBlk;

const BLOCK_SIZE: usize = 4096;

const SUPERBLOCK_BLOCK: u64 = 0;
const INODE_BITMAP_BLOCK: u64 = 1;
const BLOCK_BITMAP_START: u64 = 2;
const BLOCK_BITMAP_BLOCKS: u64 = 4;

const INODE_SIZE: usize = 128;
const INODE_COUNT: u32 = 1024;

const INODE_TABLE_BLOCK: u64 = 6;
const INODE_TABLE_BLOCKS: u64 = 32;
const INODES_PER_BLOCK: usize = BLOCK_SIZE / INODE_SIZE;
const _: () = assert!(core::mem::size_of::<Inode>() == INODE_SIZE);
const DATA_START_BLOCK: u64 = 38;

const ROOT_INODE: u32 = 0;

const SUPERBLOCK_MAGIC: u32 = 0x4F_5552_4F;
const SUPERBLOCK_VERSION: u32 = 1;

const INODE_FREE: u16 = 0;
const INODE_FILE: u16 = 1;
const INODE_DIRECTORY: u16 = 2;
const DIR_ENTRY_SIZE: usize = 64;
const DIR_NAME_MAX: usize = 56;
const MAX_FILE_SIZE: usize = 12 * BLOCK_SIZE;

// Temporary filesystem scratch buffer.
//
// We are formatting during boot, before multitasking is active,
// so a single static buffer is fine for now.
//
// Later we'll replace this with a proper block-cache layer.
#[repr(align(4096))]
struct FsBlock([u8; BLOCK_SIZE]);

static mut FS_BLOCK: FsBlock = FsBlock([0; BLOCK_SIZE]);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Superblock {
    pub magic: u32,
    pub version: u32,

    pub block_size: u32,
    pub total_blocks: u64,

    pub inode_count: u32,

    pub inode_bitmap_block: u64,

    pub block_bitmap_start: u64,
    pub block_bitmap_blocks: u64,

    pub inode_table_start: u64,
    pub inode_table_blocks: u64,

    pub data_start: u64,

    pub root_inode: u32,
}

impl Superblock {
    pub fn new(total_blocks: u64) -> Self {
        Self {
            magic: SUPERBLOCK_MAGIC,
            version: SUPERBLOCK_VERSION,

            block_size: BLOCK_SIZE as u32,
            total_blocks,

            inode_count: INODE_COUNT,

            inode_bitmap_block: INODE_BITMAP_BLOCK,

            block_bitmap_start: BLOCK_BITMAP_START,
            block_bitmap_blocks: BLOCK_BITMAP_BLOCKS,

            inode_table_start: INODE_TABLE_BLOCK,
            inode_table_blocks: INODE_TABLE_BLOCKS,

            data_start: DATA_START_BLOCK,

            root_inode: ROOT_INODE,
        }
    }

    fn write_to_block(&self, block: &mut [u8; BLOCK_SIZE]) {
        block.fill(0);

        block[0..4].copy_from_slice(&self.magic.to_le_bytes());
        block[4..8].copy_from_slice(&self.version.to_le_bytes());
        block[8..12].copy_from_slice(&self.block_size.to_le_bytes());

        block[12..20].copy_from_slice(
            &self.total_blocks.to_le_bytes()
        );

        block[20..24].copy_from_slice(
            &self.inode_count.to_le_bytes()
        );

        block[24..32].copy_from_slice(
            &self.inode_bitmap_block.to_le_bytes()
        );

        block[32..40].copy_from_slice(
            &self.block_bitmap_start.to_le_bytes()
        );

        block[40..48].copy_from_slice(
            &self.block_bitmap_blocks.to_le_bytes()
        );

        block[48..56].copy_from_slice(
            &self.inode_table_start.to_le_bytes()
        );

        block[56..64].copy_from_slice(
            &self.inode_table_blocks.to_le_bytes()
        );

        block[64..72].copy_from_slice(
            &self.data_start.to_le_bytes()
        );

        block[72..76].copy_from_slice(
            &self.root_inode.to_le_bytes()
        );
    }

    fn from_block(block: &[u8; BLOCK_SIZE]) -> Self {
        Self {
            magic: u32::from_le_bytes(
                block[0..4].try_into().unwrap()
            ),

            version: u32::from_le_bytes(
                block[4..8].try_into().unwrap()
            ),

            block_size: u32::from_le_bytes(
                block[8..12].try_into().unwrap()
            ),

            total_blocks: u64::from_le_bytes(
                block[12..20].try_into().unwrap()
            ),

            inode_count: u32::from_le_bytes(
                block[20..24].try_into().unwrap()
            ),

            inode_bitmap_block: u64::from_le_bytes(
                block[24..32].try_into().unwrap()
            ),

            block_bitmap_start: u64::from_le_bytes(
                block[32..40].try_into().unwrap()
            ),

            block_bitmap_blocks: u64::from_le_bytes(
                block[40..48].try_into().unwrap()
            ),

            inode_table_start: u64::from_le_bytes(
                block[48..56].try_into().unwrap()
            ),

            inode_table_blocks: u64::from_le_bytes(
                block[56..64].try_into().unwrap()
            ),

            data_start: u64::from_le_bytes(
                block[64..72].try_into().unwrap()
            ),

            root_inode: u32::from_le_bytes(
                block[72..76].try_into().unwrap()
            ),
        }
    }

    fn valid(&self, disk_blocks: u64) -> bool {
        self.magic == SUPERBLOCK_MAGIC
            && self.version == SUPERBLOCK_VERSION
            && self.block_size == BLOCK_SIZE as u32
            && self.total_blocks == disk_blocks
            && self.inode_count == INODE_COUNT
            && self.inode_bitmap_block == INODE_BITMAP_BLOCK
            && self.block_bitmap_start == BLOCK_BITMAP_START
            && self.block_bitmap_blocks == BLOCK_BITMAP_BLOCKS
            && self.inode_table_start == INODE_TABLE_BLOCK
            && self.inode_table_blocks == INODE_TABLE_BLOCKS
            && self.data_start == DATA_START_BLOCK
            && self.root_inode == ROOT_INODE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub inode: u32,
    pub kind: FileKind,
    pub size: u64,
    pub links: u16,
}

pub struct FileSystem {
    disk: VirtioBlk,
    superblock: Superblock
}

impl FileSystem {
    pub const fn new(disk: VirtioBlk) -> Self {
        Self {
            disk,
            superblock: Superblock {
                magic: 0,
                version: 0,
                block_size: 0,
                total_blocks: 0,
                inode_count: 0,
                inode_bitmap_block: 0,
                block_bitmap_start: 0,
                block_bitmap_blocks: 0,
                inode_table_start: 0,
                inode_table_blocks: 0,
                data_start: 0,
                root_inode: 0,
            },
        }
    }
    pub fn format(&mut self) -> Result<(), ()> {
        let total_blocks = self.disk.capacity_blocks();

        // The filesystem needs at least one block beyond its metadata.
        if total_blocks <= DATA_START_BLOCK {
            return Err(());
        }

        let superblock = Superblock::new(total_blocks);

        unsafe {
            let block_ptr = &raw mut FS_BLOCK;
            let block: &mut [u8; BLOCK_SIZE] = &mut (*block_ptr).0;

            // Invalidate the old filesystem first. If formatting fails,
            // mount() must not mistake a partially formatted disk for valid.
            block.fill(0);
            self.disk
                .write_block(SUPERBLOCK_BLOCK, block)
                .map_err(|_| ())?;

            // Clear inode bitmap, block bitmaps, and inode table.
            for block_number in INODE_BITMAP_BLOCK..DATA_START_BLOCK {
                self.disk
                    .write_block(block_number, block)
                    .map_err(|_| ())?;
            }
        }

        self.superblock = superblock;
        self.initialiseRoot()?;

        // Commit the valid superblock only after metadata and root exist.
        unsafe {
            let block_ptr = &raw mut FS_BLOCK;
            let block: &mut [u8; BLOCK_SIZE] = &mut (*block_ptr).0;
            superblock.write_to_block(block);
            self.disk
                .write_block(SUPERBLOCK_BLOCK, block)
                .map_err(|_| ())?;
        }

        Ok(())
    }
    pub fn mount(&mut self) -> Result<Superblock, ()> {
        let total_blocks = self.disk.capacity_blocks();

        unsafe {
            let block_ptr = &raw mut FS_BLOCK;

            self.disk
                .read_block(SUPERBLOCK_BLOCK, &mut (*block_ptr).0)
                .map_err(|_| ())?;

            let superblock = Superblock::from_block(&(*block_ptr).0);

            if !superblock.valid(total_blocks) {
                return Err(());
            }
            self.superblock = superblock;

            Ok(superblock)
        }
    }
    fn emptyInode() -> Inode {
        Inode{
            mode: 0, links: 0, size: 0, direct: [0; 12], indirect: 0, doubleIndirect: 0, reserved: [0;52]
        }
    }
    fn inodeLocation(&self, inodeNumber: u32) -> Option<(u64, usize)> {
        if inodeNumber >= self.superblock.inode_count {
            return None;
        }

        let index = inodeNumber as usize;

        let block = self.superblock.inode_table_start
            + (index / INODES_PER_BLOCK) as u64;

        let offset = (index % INODES_PER_BLOCK) * INODE_SIZE;

        Some((block, offset))
    }
    pub fn readInode(&mut self, inodeNumber: u32) -> Result<Inode, ()> {
        let (blockNumber, offset) = self.inodeLocation(inodeNumber).ok_or(())?;

        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let block = &mut (*blockPtr).0;

            self.disk
                .read_block(blockNumber, block)
                .map_err(|_| ())?;

            let inode = core::ptr::read_unaligned(
                block[offset..offset + INODE_SIZE]
                    .as_ptr()
                    .cast::<Inode>()
            );

            Ok(inode)
        }
    }
    pub fn writeInode(&mut self, inodeNumber: u32, inode: &Inode) -> Result<(), ()>{
        let (blockNumber, offset) = self.inodeLocation(inodeNumber).ok_or(())?;
        unsafe{
            let blockPtr = &raw mut FS_BLOCK;
            let block = &mut (*blockPtr).0;
            self.disk.read_block(blockNumber, block).map_err(|_| ())?;
            core::ptr::write_unaligned(block[offset..offset +INODE_SIZE].as_mut_ptr().cast::<Inode>(), *inode);
            self.disk.write_block(blockNumber, block).map_err(|_|())?;
        }
        Ok(())
    }
    pub fn allocInode(&mut self) -> Result<u32, ()>{
        let bitmapBlock = self.superblock.inode_bitmap_block;

        let (inodeNumber, byteIndex, mask) = unsafe{
            let blockPtr = &raw mut FS_BLOCK;
            let bitmap = &mut (*blockPtr).0;
            self.disk.read_block(bitmapBlock, bitmap).map_err(|_| ())?;

            let mut found = None;
            for inodeNumber in 1..self.superblock.inode_count {
                let byteIndex = (inodeNumber / 8) as usize;
                let bitIndex = (inodeNumber % 8) as u8;
                let mask = 1u8 << bitIndex;
                if bitmap[byteIndex] & mask == 0{
                    found = Some((inodeNumber, byteIndex, mask));
                    break;
                }
            }
            found.ok_or(())?
        };
        let inode = Self::emptyInode();
        self.writeInode(inodeNumber, &inode)?;
        unsafe{
            let blockPtr = &raw mut FS_BLOCK;
            let bitmap = &mut (*blockPtr).0;
            self.disk.read_block(bitmapBlock, bitmap).map_err(|_| ())?;
            bitmap[byteIndex] |= mask;
            self.disk.write_block(bitmapBlock, bitmap).map_err(|_| ())?;
        }
        Ok(inodeNumber)
    }
    fn freeInode(&mut self, inodeNumber: u32) -> Result<(), ()> {
        if inodeNumber == ROOT_INODE
            || inodeNumber >= self.superblock.inode_count
        {
            return Err(());
        }

        let inode = self.readInode(inodeNumber)?;

        if inode.indirect != 0 || inode.doubleIndirect != 0 {
            return Err(());
        }

        let expectedBlocks = match inode.mode {
            INODE_FILE => {
                if inode.size > MAX_FILE_SIZE as u64 {
                    return Err(());
                }
                (inode.size as usize + BLOCK_SIZE - 1) / BLOCK_SIZE
            }
            // A directory may only be freed once it contains its two
            // required entries: "." and "..".
            INODE_DIRECTORY => {
                if inode.size != (2 * DIR_ENTRY_SIZE) as u64 {
                    return Err(());
                }
                1
            }
            _ => return Err(()),
        };

        if expectedBlocks > inode.direct.len() {
            return Err(());
        }

        // Check the inode bitmap before freeing any resources.
        let inodeBitmapBlock = self.superblock.inode_bitmap_block;
        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let bitmap = &mut (*blockPtr).0;
            self.disk
                .read_block(inodeBitmapBlock, bitmap)
                .map_err(|_| ())?;

            let byteIndex = (inodeNumber / 8) as usize;
            let mask = 1u8 << (inodeNumber % 8);
            if bitmap[byteIndex] & mask == 0 {
                return Err(());
            }
        }

        // Validate pointers, expected occupancy, and uniqueness before
        // releasing any block. This avoids partially freeing a corrupt inode.
        for i in 0..inode.direct.len() {
            let blockNumber = inode.direct[i] as u64;
            if i < expectedBlocks {
                if blockNumber < self.superblock.data_start
                    || blockNumber >= self.superblock.total_blocks
                {
                    return Err(());
                }
            } else if blockNumber != 0 {
                return Err(());
            }

            if blockNumber != 0 {
                for previous in 0..i {
                    if inode.direct[previous] == inode.direct[i] {
                        return Err(());
                    }
                }
            }
        }

        for i in 0..expectedBlocks {
            self.freeBlock(inode.direct[i] as u64)?;
        }

        self.writeInode(inodeNumber, &Self::emptyInode())?;

        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let bitmap = &mut (*blockPtr).0;
            self.disk
                .read_block(inodeBitmapBlock, bitmap)
                .map_err(|_| ())?;

            let byteIndex = (inodeNumber / 8) as usize;
            let mask = 1u8 << (inodeNumber % 8);
            if bitmap[byteIndex] & mask == 0 {
                return Err(());
            }

            bitmap[byteIndex] &= !mask;
            self.disk
                .write_block(inodeBitmapBlock, bitmap)
                .map_err(|_| ())?;
        }

        Ok(())
    }
    pub fn allocBlock(&mut self) -> Result<u64, ()> {
        let bitmapStart = self.superblock.block_bitmap_start;
        let dataStart = self.superblock.data_start;
        let totalBlocks = self.superblock.total_blocks;

        for blockNumber in dataStart..totalBlocks{
            let byteIndex = (blockNumber / 8) as usize;
            let bitIndex = (blockNumber % 8) as u8;
            let bitmapBlock = bitmapStart + (byteIndex / BLOCK_SIZE) as u64;
            let bitmapOffset = byteIndex % BLOCK_SIZE;
            unsafe{
                let blockPtr = &raw mut FS_BLOCK;
                let bitmap = &mut (*blockPtr).0;
                self.disk.read_block(bitmapBlock, bitmap).map_err(|_| ())?;
                let mask = 1u8 << bitIndex;
                if bitmap[bitmapOffset] & mask != 0{
                    continue;
                }
                bitmap[bitmapOffset] |= mask;
                self.disk.write_block(bitmapBlock, bitmap).map_err(|_| ())?;
                bitmap.fill(0);
                self.disk.write_block(blockNumber, bitmap).map_err(|_| ())?;

                return Ok(blockNumber);

            }
        }
        Err(())
    }

    pub fn disk_usage(&mut self) -> Result<(u64, u64, u64, u64), ()> {
        let total_data_blocks = self.superblock.total_blocks.saturating_sub(self.superblock.data_start);
        let mut used_blocks = 0u64;
        let mut bit_index = 0usize;

        for bitmap_block_index in 0..self.superblock.block_bitmap_blocks {
            let bitmap_block = self.superblock.block_bitmap_start + bitmap_block_index;
            let mut bitmap = [0u8; BLOCK_SIZE];

            self.disk.read_block(bitmap_block, &mut bitmap).map_err(|_| ())?;

            for byte in bitmap.iter() {
                if bit_index >= total_data_blocks as usize {
                    let free_blocks = total_data_blocks.saturating_sub(used_blocks);
                    return Ok((total_data_blocks, used_blocks, free_blocks, BLOCK_SIZE as u64));
                }

                for bit in 0..8 {
                    if bit_index >= total_data_blocks as usize {
                        let free_blocks = total_data_blocks.saturating_sub(used_blocks);
                        return Ok((total_data_blocks, used_blocks, free_blocks, BLOCK_SIZE as u64));
                    }

                    if byte & (1u8 << bit) != 0 {
                        used_blocks += 1;
                    }
                    bit_index += 1;
                }
            }
        }

        let free_blocks = total_data_blocks.saturating_sub(used_blocks);
        Ok((total_data_blocks, used_blocks, free_blocks, BLOCK_SIZE as u64))
    }
    fn freeBlock(&mut self, blockNumber: u64) -> Result<(), ()> {
        if blockNumber < self.superblock.data_start
            || blockNumber >= self.superblock.total_blocks
        {
            return Err(());
        }

        let byteIndex = (blockNumber / 8) as usize;
        let bitIndex = (blockNumber % 8) as u8;

        let bitmapBlock = self.superblock.block_bitmap_start
            + (byteIndex / BLOCK_SIZE) as u64;

        let bitmapOffset = byteIndex % BLOCK_SIZE;
        let mask = 1u8 << bitIndex;

        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let bitmap = &mut (*blockPtr).0;

            self.disk
                .read_block(bitmapBlock, bitmap)
                .map_err(|_| ())?;

            // Refuse to free an already-free block.
            if bitmap[bitmapOffset] & mask == 0 {
                return Err(());
            }

            bitmap[bitmapOffset] &= !mask;

            self.disk
                .write_block(bitmapBlock, bitmap)
                .map_err(|_| ())?;
        }

        Ok(())
}
    /// Replace a regular file's complete contents. Empty data truncates the file.
    /// Files are limited to twelve direct data blocks in this format version.
    pub fn writeFile(&mut self, inodeNumber: u32, data: &[u8]) -> Result<usize, ()> {
        if data.len() > MAX_FILE_SIZE {
            return Err(());
        }

        let mut inode = self.readInode(inodeNumber)?;
        if inode.mode != INODE_FILE
            || inode.indirect != 0
            || inode.doubleIndirect != 0
            || inode.size > MAX_FILE_SIZE as u64
        {
            return Err(());
        }

        let oldBlockCount =
            (inode.size as usize + BLOCK_SIZE - 1) / BLOCK_SIZE;
        let newBlockCount = (data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;

        // Validate the file's existing direct-pointer layout before reusing it.
        for i in 0..inode.direct.len() {
            let blockNumber = inode.direct[i] as u64;
            if i < oldBlockCount {
                if blockNumber < self.superblock.data_start
                    || blockNumber >= self.superblock.total_blocks
                {
                    return Err(());
                }
            } else if blockNumber != 0 {
                return Err(());
            }

            if blockNumber != 0 {
                for previous in 0..i {
                    if inode.direct[previous] == inode.direct[i] {
                        return Err(());
                    }
                }
            }
        }

        let oldDirect = inode.direct;
        let mut newDirect = [0u32; 12];
        let reusedCount = core::cmp::min(oldBlockCount, newBlockCount);
        newDirect[..reusedCount].copy_from_slice(&oldDirect[..reusedCount]);

        // Allocate only additional blocks when growing. Existing blocks are
        // reused, so replacing a file does not require a second full copy
        // of its storage to be available at once.
        let mut allocatedCount = 0usize;
        for i in oldBlockCount..newBlockCount {
            let blockNumber = match self.allocBlock() {
                Ok(blockNumber) => blockNumber,
                Err(()) => {
                    for j in oldBlockCount..(oldBlockCount + allocatedCount) {
                        let _ = self.freeBlock(newDirect[j] as u64);
                    }
                    return Err(());
                }
            };

            if blockNumber > u32::MAX as u64 {
                let _ = self.freeBlock(blockNumber);
                for j in oldBlockCount..(oldBlockCount + allocatedCount) {
                    let _ = self.freeBlock(newDirect[j] as u64);
                }
                return Err(());
            }

            newDirect[i] = blockNumber as u32;
            allocatedCount += 1;
        }

        // Write each block, clearing its unused tail. If a device write fails,
        // release newly allocated blocks; reused blocks may already have been
        // overwritten, so this is not a crash-atomic filesystem transaction.
        for i in 0..newBlockCount {
            let blockNumber = newDirect[i] as u64;
            let start = i * BLOCK_SIZE;
            let end = core::cmp::min(start + BLOCK_SIZE, data.len());
            let writeResult = unsafe {
                let blockPtr = &raw mut FS_BLOCK;
                let block = &mut (*blockPtr).0;
                block.fill(0);
                block[..end - start].copy_from_slice(&data[start..end]);
                self.disk.write_block(blockNumber, block).map_err(|_| ())
            };

            if writeResult.is_err() {
                for j in oldBlockCount..oldBlockCount + allocatedCount {
                    let _ = self.freeBlock(newDirect[j] as u64);
                }
                return Err(());
            }
        }

        inode.direct = newDirect;
        inode.size = data.len() as u64;
        self.writeInode(inodeNumber, &inode)?;

        // Only blocks no longer referenced by the new inode are released.
        for i in newBlockCount..oldBlockCount {
            self.freeBlock(oldDirect[i] as u64)?;
        }

        Ok(data.len())
    }
    pub fn readFile(&mut self, inodeNumber: u32, buffer: &mut [u8]) -> Result<usize, ()>{
        let inode = self.readInode(inodeNumber)?;

        if inode.mode != INODE_FILE
            || inode.size > MAX_FILE_SIZE as u64
            || inode.indirect != 0
            || inode.doubleIndirect != 0
        {
            return Err(());
        }

        let size = inode.size as usize;
        if size > buffer.len() {
            return Err(());
        }

        let blockCount = (size + BLOCK_SIZE - 1) / BLOCK_SIZE;
        for i in 0..blockCount{
            let blockNumber = inode.direct[i] as u64;
            if blockNumber < self.superblock.data_start
                || blockNumber >= self.superblock.total_blocks
            {
                return Err(());
            }
            let start = i * BLOCK_SIZE;
            let end = core::cmp::min(start + BLOCK_SIZE, size);
            unsafe{
                let blockPtr = &raw mut FS_BLOCK;
                let block = &mut (*blockPtr).0;
                self.disk.read_block(blockNumber, block).map_err(|_| ())?;
                buffer[start..end].copy_from_slice(&block[..end - start]);
            }
        }        
        Ok(size)
    }
    pub fn createFile(&mut self, data: &[u8]) -> Result<u32, ()> {
        let inodeNumber = self.allocInode()?;

        let mut inode = self.readInode(inodeNumber)?;

        inode.mode = INODE_FILE;
        inode.links = 1;

        self.writeInode(inodeNumber, &inode)?;

        if self.writeFile(inodeNumber, data).is_err() {
            let _ = self.freeInode(inodeNumber);
            return Err(());
        }

        Ok(inodeNumber)
    }
    pub fn createFileInDirectory(
        &mut self,
        parentInode: u32,
        name: &[u8],
        data: &[u8],
    ) -> Result<u32, ()> {
        // Validate the filename.
        if name.is_empty()
            || name.len() > DIR_NAME_MAX
            || name.contains(&b'/')
            || name.contains(&0)
            || name == b"."
            || name == b".."
        {
            return Err(());
        }

        // The parent must be a directory.
        let parent = self.readInode(parentInode)?;

        if parent.mode != INODE_DIRECTORY {
            return Err(());
        }

        // Don't allow duplicate names.
        if self.findDirectoryEntry(parentInode, name)?.is_some() {
            return Err(());
        }

        // Create the file and write its contents.
        let inodeNumber = self.createFile(data)?;

        // Connect the filename to the new inode.
        let entry = Self::makeDirEntry(
            inodeNumber,
            INODE_FILE as u8,
            name,
        )?;

        // If insertion fails, release the newly created file.
        if self.addDirectoryEntry(parentInode, &entry).is_err() {
            self.freeInode(inodeNumber)?;
            return Err(());
        }

        Ok(inodeNumber)
    }
    pub fn deleteFile(
        &mut self,
        parentInode: u32,
        name: &[u8],
    ) -> Result<(), ()> {
        let parent = self.readInode(parentInode)?;

        if parent.mode != INODE_DIRECTORY {
            return Err(());
        }

        let fileInode = self
            .findDirectoryEntry(parentInode, name)?
            .ok_or(())?;

        let inode = self.readInode(fileInode)?;

        // Don't let this operation delete directories.
        if inode.mode != INODE_FILE {
            return Err(());
        }

        // Remove the name first, then release the file's resources.
        let removedInode =
            self.removeDirectoryEntry(parentInode, name)?;

        if removedInode != fileInode {
            return Err(());
        }

        self.freeInode(fileInode)?;

        Ok(())
    }

    /// Remove an empty child directory from a parent directory.
    pub fn removeDirectory(
        &mut self,
        parentInode: u32,
        name: &[u8],
    ) -> Result<(), ()> {
        if name.is_empty()
            || name.len() > DIR_NAME_MAX
            || name.contains(&b'/')
            || name.contains(&0)
            || name == b"."
            || name == b".."
        {
            return Err(());
        }

        let parent = self.readInode(parentInode)?;
        if parent.mode != INODE_DIRECTORY || parent.links <= 2 {
            return Err(());
        }

        let directoryInode = self
            .findDirectoryEntry(parentInode, name)?
            .ok_or(())?;

        if directoryInode == ROOT_INODE {
            return Err(());
        }

        let directory = self.readInode(directoryInode)?;
        if directory.mode != INODE_DIRECTORY
            || directory.size != (2 * DIR_ENTRY_SIZE) as u64
            || directory.indirect != 0
            || directory.doubleIndirect != 0
        {
            return Err(());
        }

        // Ensure the target is actually an empty directory whose dot entries
        // identify itself and the supplied parent.
        if self.findDirectoryEntry(directoryInode, b".")? != Some(directoryInode)
            || self.findDirectoryEntry(directoryInode, b"..")? != Some(parentInode)
        {
            return Err(());
        }

        let blockNumber = directory.direct[0] as u64;
        if blockNumber < self.superblock.data_start
            || blockNumber >= self.superblock.total_blocks
            || directory.direct[1..].iter().any(|&block| block != 0)
        {
            return Err(());
        }

        let removedInode = self.removeDirectoryEntry(parentInode, name)?;
        if removedInode != directoryInode {
            return Err(());
        }

        let mut parent = self.readInode(parentInode)?;
        if parent.links <= 2 {
            return Err(());
        }
        parent.links -= 1;
        self.writeInode(parentInode, &parent)?;

        self.freeInode(directoryInode)?;
        Ok(())
    }

    fn makeDirEntry(inodeNumber: u32, kind:u8, name: &[u8]) -> Result<DirEntry, ()>{
        if name.is_empty()
            || name.len() > DIR_NAME_MAX
            || name.contains(&b'/')
            || name.contains(&0)
        {
            return Err(());
        }
        let mut entry = DirEntry{
            inode: inodeNumber,
            kind,
            nameLength: name.len() as u8,
            reserved: 0,
            name: [0; DIR_NAME_MAX]
        };
        entry.name[..name.len()].copy_from_slice(name);
        Ok(entry)
    }
    pub fn addDirectoryEntry(&mut self, directoryInode: u32, entry: &DirEntry) -> Result<(), ()>{
        let mut inode = self.readInode(directoryInode)?;
        if inode.mode != INODE_DIRECTORY
            || inode.size % DIR_ENTRY_SIZE as u64 != 0
            || inode.size > (inode.direct.len() * BLOCK_SIZE) as u64
            || entry.nameLength == 0
            || entry.nameLength as usize > DIR_NAME_MAX
        {
            return Err(());
        }

        let entryIndex = inode.size as usize / DIR_ENTRY_SIZE;
        let blockIndex = entryIndex / (BLOCK_SIZE / DIR_ENTRY_SIZE);
        let offset = (entryIndex % (BLOCK_SIZE / DIR_ENTRY_SIZE)) * DIR_ENTRY_SIZE;

        if blockIndex >= inode.direct.len(){
            return Err(());
        }
        if offset == 0 && inode.direct[blockIndex] == 0 {
            let blockNumber = self.allocBlock()?;
            inode.direct[blockIndex] = blockNumber as u32;
        }

        let blockNumber = inode.direct[blockIndex] as u64;
        if blockNumber < self.superblock.data_start
            || blockNumber >= self.superblock.total_blocks
        {
            return Err(());
        }

        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let block = &mut (*blockPtr).0;

            self.disk
                .read_block(blockNumber, block)
                .map_err(|_| ())?;

            core::ptr::write_unaligned(
                block[offset..offset + DIR_ENTRY_SIZE]
                    .as_mut_ptr()
                    .cast::<DirEntry>(),
                *entry,
            );

            self.disk
                .write_block(blockNumber, block)
                .map_err(|_| ())?;
        }

        inode.size += DIR_ENTRY_SIZE as u64;

        self.writeInode(directoryInode, &inode)?;
        Ok(())
    }
    fn removeDirectoryEntry(
        &mut self,
        directoryInode: u32,
        name: &[u8],
    ) -> Result<u32, ()> {
        if name.is_empty()
            || name.len() > DIR_NAME_MAX
            || name == b"."
            || name == b".."
            || name.contains(&b'/')
            || name.contains(&0)
        {
            return Err(());
        }

        let directory = self.readInode(directoryInode)?;

        if directory.mode != INODE_DIRECTORY
            || directory.size % DIR_ENTRY_SIZE as u64 != 0
        {
            return Err(());
        }

        let entryCount =
            directory.size as usize / DIR_ENTRY_SIZE;

        let mut found = None;

        // Locate the entry to remove.
        for index in 0..entryCount {
            let blockIndex =
                index / (BLOCK_SIZE / DIR_ENTRY_SIZE);

            let offset =
                (index % (BLOCK_SIZE / DIR_ENTRY_SIZE))
                    * DIR_ENTRY_SIZE;

            if blockIndex >= directory.direct.len() {
                return Err(());
            }

            let blockNumber = directory.direct[blockIndex] as u64;

            if blockNumber == 0 {
                return Err(());
            }

            let entry = unsafe {
                let blockPtr = &raw mut FS_BLOCK;
                let block = &mut (*blockPtr).0;

                self.disk
                    .read_block(blockNumber, block)
                    .map_err(|_| ())?;

                core::ptr::read_unaligned(
                    block[offset..offset + DIR_ENTRY_SIZE]
                        .as_ptr()
                        .cast::<DirEntry>(),
                )
            };

            if entry.nameLength as usize == name.len()
                && &entry.name[..name.len()] == name
            {
                found = Some((index, entry.inode));
                break;
            }
        }

        let (targetIndex, removedInode) = found.ok_or(())?;

        let lastIndex = entryCount.checked_sub(1).ok_or(())?;

        let targetBlockIndex =
            targetIndex / (BLOCK_SIZE / DIR_ENTRY_SIZE);

        let targetOffset =
            (targetIndex % (BLOCK_SIZE / DIR_ENTRY_SIZE))
                * DIR_ENTRY_SIZE;

        let lastBlockIndex =
            lastIndex / (BLOCK_SIZE / DIR_ENTRY_SIZE);

        let lastOffset =
            (lastIndex % (BLOCK_SIZE / DIR_ENTRY_SIZE))
                * DIR_ENTRY_SIZE;

        if targetBlockIndex >= directory.direct.len()
            || lastBlockIndex >= directory.direct.len()
        {
            return Err(());
        }

        let targetBlockNumber =
            directory.direct[targetBlockIndex] as u64;

        let lastBlockNumber =
            directory.direct[lastBlockIndex] as u64;

        if targetBlockNumber == 0 || lastBlockNumber == 0 {
            return Err(());
        }

        // Preserve the final entry before reusing the scratch buffer.
        let lastEntry = unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let block = &mut (*blockPtr).0;

            self.disk
                .read_block(lastBlockNumber, block)
                .map_err(|_| ())?;

            core::ptr::read_unaligned(
                block[lastOffset..lastOffset + DIR_ENTRY_SIZE]
                    .as_ptr()
                    .cast::<DirEntry>(),
            )
        };

        // Move the last entry into the removed entry's position.
        if targetIndex != lastIndex {
            unsafe {
                let blockPtr = &raw mut FS_BLOCK;
                let block = &mut (*blockPtr).0;

                self.disk
                    .read_block(targetBlockNumber, block)
                    .map_err(|_| ())?;

                core::ptr::write_unaligned(
                    block[targetOffset..targetOffset + DIR_ENTRY_SIZE]
                        .as_mut_ptr()
                        .cast::<DirEntry>(),
                    lastEntry,
                );

                self.disk
                    .write_block(targetBlockNumber, block)
                    .map_err(|_| ())?;
            }
        }

        // Clear the now-unused final entry on disk.
        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let block = &mut (*blockPtr).0;

            self.disk
                .read_block(lastBlockNumber, block)
                .map_err(|_| ())?;

            block[lastOffset..lastOffset + DIR_ENTRY_SIZE].fill(0);

            self.disk
                .write_block(lastBlockNumber, block)
                .map_err(|_| ())?;
        }

        let mut directory = directory;
        directory.size -= DIR_ENTRY_SIZE as u64;

        // If the removed entry was the only entry in the final block,
        // detach and free that block. Block zero is retained for every
        // valid directory because it contains at least "." and "..".
        let entries_per_block = BLOCK_SIZE / DIR_ENTRY_SIZE;
        let block_to_free = if lastIndex != 0
            && lastIndex % entries_per_block == 0
        {
            let block_number = directory.direct[lastBlockIndex] as u64;
            directory.direct[lastBlockIndex] = 0;
            Some(block_number)
        } else {
            None
        };

        // Remove the reference before returning the block to the allocator.
        self.writeInode(directoryInode, &directory)?;

        if let Some(block_number) = block_to_free {
            self.freeBlock(block_number)?;
        }

        Ok(removedInode)
    }
    pub fn createDirectory(&mut self, parentInode: u32, name: &[u8],) -> Result<u32, ()> {
        let parent = self.readInode(parentInode)?;

        if parent.mode != INODE_DIRECTORY {
            return Err(());
        }
        if name.is_empty()
            || name.len() > DIR_NAME_MAX
            || name.contains(&b'/')
            || name.contains(&0)
            || name == b"."
            || name == b".."
        {
            return Err(());
        }

        // Do not allow duplicate names in the same directory.
        if self.findDirectoryEntry(parentInode, name)?.is_some() {
            return Err(());
        }

        // Check capacity before allocating the child inode and data block.
        // Otherwise a full parent directory could leak those allocations.
        if parent.size % DIR_ENTRY_SIZE as u64 != 0
            || parent.size >= (parent.direct.len() * BLOCK_SIZE) as u64
        {
            return Err(());
        }

        let inodeNumber = self.allocInode()?;
        let blockNumber = self.allocBlock()?;

        let dot = Self::makeDirEntry(
            inodeNumber,
            INODE_DIRECTORY as u8,
            b".",
        )?;

        let dotDot = Self::makeDirEntry(
            parentInode,
            INODE_DIRECTORY as u8,
            b"..",
        )?;

        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let block = &mut (*blockPtr).0;

            block.fill(0);

            core::ptr::write_unaligned(
                block[0..DIR_ENTRY_SIZE]
                    .as_mut_ptr()
                    .cast::<DirEntry>(),
                dot,
            );

            core::ptr::write_unaligned(
                block[DIR_ENTRY_SIZE..DIR_ENTRY_SIZE * 2]
                    .as_mut_ptr()
                    .cast::<DirEntry>(),
                dotDot,
            );

            self.disk
                .write_block(blockNumber, block)
                .map_err(|_| ())?;
        }

        let mut inode = self.readInode(inodeNumber)?;

        inode.mode = INODE_DIRECTORY;
        inode.links = 2;
        inode.size = (DIR_ENTRY_SIZE * 2) as u64;
        inode.direct[0] = blockNumber as u32;

        self.writeInode(inodeNumber, &inode)?;

        let entry = Self::makeDirEntry(
            inodeNumber,
            INODE_DIRECTORY as u8,
            name,
        )?;

        self.addDirectoryEntry(
            parentInode,
            &entry,
        )?;

        let mut parent = self.readInode(parentInode)?;
        parent.links += 1;

        self.writeInode(parentInode, &parent)?;

        Ok(inodeNumber)
    }
    pub fn findDirectoryEntry(&mut self,directoryInode: u32,name: &[u8],) -> Result<Option<u32>, ()> {
        if name.is_empty()
            || name.len() > DIR_NAME_MAX
            || name.contains(&0)
            || name.contains(&b'/')
        {
            return Err(());
        }

        let inode = self.readInode(directoryInode)?;

        if inode.mode != INODE_DIRECTORY
            || inode.size % DIR_ENTRY_SIZE as u64 != 0
            || inode.size > (inode.direct.len() * BLOCK_SIZE) as u64
        {
            return Err(());
        }

        let entryCount = inode.size as usize / DIR_ENTRY_SIZE;

	for i in 0..entryCount {
		let blockIndex =
			i / (BLOCK_SIZE / DIR_ENTRY_SIZE);

		let offset =
			(i % (BLOCK_SIZE / DIR_ENTRY_SIZE))
			* DIR_ENTRY_SIZE;

		if blockIndex >= inode.direct.len() {
			return Err(());
		}

            let blockNumber = inode.direct[blockIndex] as u64;
            if blockNumber < self.superblock.data_start
                || blockNumber >= self.superblock.total_blocks
            {
                return Err(());
            }

            unsafe {
                let blockPtr = &raw mut FS_BLOCK;
			let block = &mut (*blockPtr).0;

			self.disk
				.read_block(blockNumber, block)
				.map_err(|_| ())?;

			let entry = core::ptr::read_unaligned(
				block[offset..offset + DIR_ENTRY_SIZE]
					.as_ptr()
					.cast::<DirEntry>()
			);

                let entryNameLength = entry.nameLength as usize;
                if entryNameLength > DIR_NAME_MAX
                    || entry.inode >= self.superblock.inode_count
                {
                    return Err(());
                }

                if entryNameLength == name.len()
                    && &entry.name[..name.len()] == name
                {
                    return Ok(Some(entry.inode));
                }
            }
        }

        Ok(None)
    }
    fn reserveInode(&mut self, inodeNumber: u32) -> Result<(), ()> {
        let bitmapBlock = self.superblock.inode_bitmap_block;

        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let bitmap = &mut (*blockPtr).0;

            self.disk
                .read_block(bitmapBlock, bitmap)
                .map_err(|_| ())?;

            let byteIndex = (inodeNumber / 8) as usize;
            let bitIndex = (inodeNumber % 8) as u8;

            bitmap[byteIndex] |= 1u8 << bitIndex;

            self.disk
                .write_block(bitmapBlock, bitmap)
                .map_err(|_| ())?;
        }

        Ok(())
    }
    fn initialiseRoot(&mut self) -> Result<(), ()> {
        self.reserveInode(ROOT_INODE)?;

        let blockNumber = self.allocBlock()?;

        let dot = Self::makeDirEntry(
            ROOT_INODE,
            INODE_DIRECTORY as u8,
            b".",
        )?;

        let dotDot = Self::makeDirEntry(
            ROOT_INODE,
            INODE_DIRECTORY as u8,
            b"..",
        )?;

        unsafe {
            let blockPtr = &raw mut FS_BLOCK;
            let block = &mut (*blockPtr).0;

            block.fill(0);

            core::ptr::write_unaligned(
                block[0..DIR_ENTRY_SIZE]
                    .as_mut_ptr()
                    .cast::<DirEntry>(),
                dot,
            );

            core::ptr::write_unaligned(
                block[DIR_ENTRY_SIZE..DIR_ENTRY_SIZE * 2]
                    .as_mut_ptr()
                    .cast::<DirEntry>(),
                dotDot,
            );

            self.disk
                .write_block(blockNumber, block)
                .map_err(|_| ())?;
        }

        let root = Inode {
            mode: INODE_DIRECTORY,
            links: 2,
            size: (DIR_ENTRY_SIZE * 2) as u64,
            direct: {
                let mut direct = [0; 12];
                direct[0] = blockNumber as u32;
                direct
            },
            indirect: 0,
            doubleIndirect: 0,
            reserved: [0; 52],
        };

        self.writeInode(ROOT_INODE, &root)?;

        Ok(())
    }
    pub fn resolvePath(&mut self, path: &[u8]) -> Result<u32, ()> {
        if path.is_empty()
            || path[0] != b'/'
            || path.contains(&0)
        {
            return Err(());
        }

        let mut currentInode = ROOT_INODE;

        for component in path.split(|byte| *byte == b'/') {
            // Allow repeated slashes and trailing slashes.
            if component.is_empty() {
                continue;
            }

            if component.len() > DIR_NAME_MAX {
                return Err(());
            }

            // Every component must be reached through a directory.
            let current = self.readInode(currentInode)?;

            if current.mode != INODE_DIRECTORY {
                return Err(());
            }

            // "." means the current directory.
            if component == b"." {
                continue;
            }

            // ".." is resolved through its directory entry.
            let nextInode = self
                .findDirectoryEntry(currentInode, component)?
                .ok_or(())?;

            let next = self.readInode(nextInode)?;

            if next.mode == INODE_FREE {
                return Err(());
            }

            currentInode = nextInode;
        }

        // A path ending in "/" must refer to a directory.
        if path.last() == Some(&b'/') {
            let inode = self.readInode(currentInode)?;

            if inode.mode != INODE_DIRECTORY {
                return Err(());
            }
        }

        Ok(currentInode)
    }
    pub fn deleteFilePath(&mut self, path: &[u8]) -> Result<(), ()> {
        if path.len() < 2
            || path[0] != b'/'
            || path[path.len() - 1] == b'/'
            || path.contains(&0)
        {
            return Err(());
        }

        let slashIndex = path
            .iter()
            .rposition(|byte| *byte == b'/')
            .ok_or(())?;

        let name = &path[slashIndex + 1..];

        let parentPath: &[u8] = if slashIndex == 0 {
            b"/"
        } else {
            &path[..slashIndex]
        };

        let parentInode = self.resolvePath(parentPath)?;

        self.deleteFile(parentInode, name)
    }

    /// Remove an empty directory by absolute path. Root cannot be removed.
    pub fn removeDirectoryPath(&mut self, path: &[u8]) -> Result<(), ()> {
        let (parentPath, name) = Self::splitParentAndName(path)?;
        let parentInode = self.resolvePath(parentPath)?;
        self.removeDirectory(parentInode, name)
    }

    pub fn listDirectory<F>(
        &mut self,
        directoryInode: u32,
        mut visitor: F,
    ) -> Result<usize, ()>
    where
        F: FnMut(&[u8], u32, u8),
    {
        let directory = self.readInode(directoryInode)?;

        if directory.mode != INODE_DIRECTORY {
            return Err(());
        }

        let entrySize = core::mem::size_of::<DirEntry>();

        // Directories currently use direct pointers only.
        let maxSize = (directory.direct.len() * BLOCK_SIZE) as u64;

        if directory.size > maxSize
            || directory.size % entrySize as u64 != 0
        {
            return Err(());
        }

        let entriesPerBlock = BLOCK_SIZE / entrySize;
        let entryCount = directory.size as usize / entrySize;

        for index in 0..entryCount {
            let blockIndex = index / entriesPerBlock;

            if blockIndex >= directory.direct.len() {
                return Err(());
            }

            let blockNumber = directory.direct[blockIndex] as u64;

            if blockNumber < self.superblock.data_start
                || blockNumber >= self.superblock.total_blocks
            {
                return Err(());
            }

            let entryOffset = (index % entriesPerBlock) * entrySize;

            // Copy the useful data out of the shared filesystem buffer.
            // The callback must not receive a reference into FS_BLOCK.
            let (name, nameLength, entryInode, entryKind) = unsafe {
                let blockPtr = &raw mut FS_BLOCK;

                self.disk
                    .read_block(blockNumber, &mut (*blockPtr).0)
                    .map_err(|_| ())?;

                let entry = core::ptr::read_unaligned(
                    (*blockPtr)
                        .0
                        .as_ptr()
                        .add(entryOffset)
                        .cast::<DirEntry>(),
                );

                let nameLength = entry.nameLength as usize;

                if nameLength == 0
                    || nameLength > entry.name.len()
                    || entry.inode >= self.superblock.inode_count
                {
                    return Err(());
                }

                let mut name = [0u8; 56];

                name[..nameLength]
                    .copy_from_slice(&entry.name[..nameLength]);

                (name, nameLength, entry.inode, entry.kind)
            };

            visitor(
                &name[..nameLength],
                entryInode,
                entryKind,
            );
        }

        Ok(entryCount)
    }
    fn splitParentAndName(path: &[u8]) -> Result<(&[u8], &[u8]), ()> {
        if path.len() < 2 || path[0] != b'/' || path.contains(&0) {
            return Err(());
        }

        if path[path.len() - 1] == b'/' {
            return Err(());
        }

        let separator = path
            .iter()
            .rposition(|&byte| byte == b'/')
            .ok_or(())?;

        let name = &path[separator + 1..];

        if name.is_empty()
            || name.len() > 56
            || name == b"."
            || name == b".."
        {
            return Err(());
        }

        let parentPath = if separator == 0 {
            &path[..1]
        } else {
            &path[..separator]
        };

        Ok((parentPath, name))
    }
    pub fn createFilePath(
        &mut self,
        path: &[u8],
        data: &[u8],
    ) -> Result<u32, ()> {
        let (parentPath, name) =
            Self::splitParentAndName(path)?;

        let parentInode = self.resolvePath(parentPath)?;

        self.createFileInDirectory(
            parentInode,
            name,
            data,
        )
    }
    pub fn createDirectoryPath(
        &mut self,
        path: &[u8],
    ) -> Result<u32, ()> {
        let (parentPath, name) =
            Self::splitParentAndName(path)?;

        let parentInode = self.resolvePath(parentPath)?;

        self.createDirectory(parentInode, name)
    }
    pub fn statPath(&mut self, path: &[u8]) -> Result<FileStat, ()> {
        let inodeNumber = self.resolvePath(path)?;
        let inode = self.readInode(inodeNumber)?;

        let kind = match inode.mode {
            INODE_FILE => FileKind::File,
            INODE_DIRECTORY => FileKind::Directory,
            _ => return Err(()),
        };

        Ok(FileStat {
            inode: inodeNumber,
            kind,
            size: inode.size,
            links: inode.links,
        })
    }

    pub fn readFilePath(
        &mut self,
        path: &[u8],
        buffer: &mut [u8],
    ) -> Result<usize, ()> {
        let fileInode = self.resolvePath(path)?;

        let inode = self.readInode(fileInode)?;

        if inode.mode != INODE_FILE {
            return Err(());
        }

        self.readFile(fileInode, buffer)
    }

    /// Replace a regular file's complete contents by absolute path.
    /// Passing an empty slice truncates it to zero bytes.
    pub fn writeFilePath(
        &mut self,
        path: &[u8],
        data: &[u8],
    ) -> Result<usize, ()> {
        let fileInode = self.resolvePath(path)?;
        let inode = self.readInode(fileInode)?;
        if inode.mode != INODE_FILE {
            return Err(());
        }
        self.writeFile(fileInode, data)
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Inode{
    mode: u16,
    links: u16,
    size: u64,
    direct: [u32; 12],
    indirect: u32,
    doubleIndirect: u32,
    reserved: [u8; 52]
}

#[repr(C)]
#[derive(Clone,Copy)]
pub struct DirEntry{
    inode: u32,
    kind: u8,
    nameLength: u8,
    reserved: u16,
    name: [u8; DIR_NAME_MAX]
}
const _: () = assert!(core::mem::size_of::<DirEntry>() == DIR_ENTRY_SIZE);