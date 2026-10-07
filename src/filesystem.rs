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

    unsafe {
        let block_ptr = &raw mut FS_BLOCK;
        let block: &mut [u8; BLOCK_SIZE] = &mut (*block_ptr).0;

        block.fill(0);

        block[0..4].copy_from_slice(&SUPERBLOCK_MAGIC.to_le_bytes());
        block[4..8].copy_from_slice(&SUPERBLOCK_VERSION.to_le_bytes());
        block[8..12].copy_from_slice(&(BLOCK_SIZE as u32).to_le_bytes());

        block[12..20].copy_from_slice(&total_blocks.to_le_bytes());
        block[20..24].copy_from_slice(&INODE_COUNT.to_le_bytes());

        block[24..32].copy_from_slice(&INODE_BITMAP_BLOCK.to_le_bytes());
        block[32..40].copy_from_slice(&BLOCK_BITMAP_START.to_le_bytes());
        block[40..48].copy_from_slice(&BLOCK_BITMAP_BLOCKS.to_le_bytes());

        block[48..56].copy_from_slice(&INODE_TABLE_BLOCK.to_le_bytes());
        block[56..64].copy_from_slice(&INODE_TABLE_BLOCKS.to_le_bytes());

        block[64..72].copy_from_slice(&DATA_START_BLOCK.to_le_bytes());
        block[72..76].copy_from_slice(&ROOT_INODE.to_le_bytes());

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
            for inodeNumber in 0..self.superblock.inode_count {
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
            self.disk.read_block(bitmapBlock, bitmap).map_err(|_| ());
            bitmap[byteIndex] |= mask;
            self.disk.write_block(bitmapBlock, bitmap).map_err(|_| ())?;
        }
        Ok(inodeNumber)
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
    pub fn writeFile(&mut self, inodeNumber: u32, data: &[u8]) -> Result<usize, ()>{
        const MAX_SIZE: usize = 12 * BLOCK_SIZE;
        if data.len() > MAX_SIZE {
            return Err(());
        }
        let mut inode = self.readInode(inodeNumber)?;
        if inode.size != 0{
            return Err(());
        }
        if inode.mode != INODE_FILE {
            return Err(());
        }
        let blockCount = (data.len() + BLOCK_SIZE -1) / BLOCK_SIZE;
        for i in 0..blockCount{
            let blockNumber = self.allocBlock()?;
            let start = i * BLOCK_SIZE;
            let end = core::cmp::min(start + BLOCK_SIZE, data.len());
            unsafe{
                let blockPtr = &raw mut FS_BLOCK;
                let block = &mut (*blockPtr).0;

                block.fill(0);
                block[..end - start].copy_from_slice(&data[start..end]);

                self.disk
                    .write_block(blockNumber, block)
                    .map_err(|_| ())?;
            }
            inode.direct[i] = blockNumber as u32;
        }
        inode.size = data.len() as u64;
        inode.links = 1;
        self.writeInode(inodeNumber, &inode)?;
        Ok(data.len())
    }
    pub fn readFile(&mut self, inodeNumber: u32, buffer: &mut [u8]) -> Result<usize, ()>{
        let inode = self.readInode(inodeNumber)?;
        let size = inode.size as usize;
        if size > buffer.len() { return Err(()); }
        if inode.mode != INODE_FILE {
            return Err(());
        }
        let blockCount = (size + BLOCK_SIZE - 1) / BLOCK_SIZE;
        for i in 0..blockCount{
            let blockNumber = inode.direct[i] as u64;
            if blockNumber == 0{ return Err(()); }
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

	self.writeFile(inodeNumber, data)?;

	Ok(inodeNumber)
}
    fn makeDirEntry(inodeNumber: u32, kind:u8, name: &[u8]) -> Result<DirEntry, ()>{
        if name.is_empty() || name.len() > DIR_NAME_MAX{
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
        if inode.mode != INODE_DIRECTORY{ return Err(()); }

        let entryIndex = inode.size as usize / DIR_ENTRY_SIZE;
        let blockIndex = entryIndex / (BLOCK_SIZE / DIR_ENTRY_SIZE);
        let offset = (entryIndex % (BLOCK_SIZE / DIR_ENTRY_SIZE)) * DIR_ENTRY_SIZE;

        if blockIndex >= inode.direct.len(){
            return Err(());
        }
        if offset ==0{
            let blockNumber = self.allocBlock()?;
            inode.direct[blockIndex] = blockNumber as u32;

            unsafe {
                let blockPtr = &raw mut FS_BLOCK;
                let block = &mut (*blockPtr).0;

                block.fill(0);

                self.disk
                    .write_block(blockNumber, block)
                    .map_err(|_| ())?;
            }
        }

        let blockNumber = inode.direct[blockIndex] as u64;

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
    pub fn createDirectory(&mut self, parentInode: u32, name: &[u8],) -> Result<u32, ()> {
        let parent = self.readInode(parentInode)?;

        if parent.mode != INODE_DIRECTORY {
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
	let inode = self.readInode(directoryInode)?;

	if inode.mode != INODE_DIRECTORY {
		return Err(());
	}

	let entryCount =
		inode.size as usize / DIR_ENTRY_SIZE;

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

			if entry.nameLength as usize == name.len()
				&& &entry.name[..name.len()] == name
			{
				return Ok(Some(entry.inode));
			}
		}
	}

	Ok(None)
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