//! Minimal read-only ext4 extractor: pull a single known file out of an ext4 filesystem
//! that lives at a byte offset inside a raw disk image, with no external tools.
//!
//! Pane needs the distro initramfs (with virtio-blk) to boot the base image under QEMU, but
//! the registered Pane initramfs is the custom pane-block one. The real one lives at
//! `/boot/initramfs-linux.img` inside the base image's ext4 root partition. Rather than
//! depend on WSL + e2fsprogs `debugfs`, this reads the ext4 metadata directly. It supports
//! the modern ext4 layout these images use: extent-mapped inodes (no legacy indirect
//! blocks), linear directory entries, 32- or 64-bit group descriptors. It reads blocks on
//! demand so it never loads the multi-GiB image into memory.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const EXT4_SUPER_MAGIC: u16 = 0xEF53;
const EXT4_EXTENTS_FL: u32 = 0x0008_0000;
const EXT4_EXTENT_MAGIC: u16 = 0xF30A;
const INCOMPAT_64BIT: u32 = 0x80;
const ROOT_INODE: u32 = 2;

/// Geometry + feature flags read from the ext4 superblock.
struct SuperBlock {
    block_size: u64,
    inodes_per_group: u32,
    inode_size: u64,
    first_data_block: u32,
    desc_size: u64,
    is_64bit: bool,
}

struct Ext4Reader {
    file: File,
    part_offset: u64,
    sb: SuperBlock,
}

fn rd_u16(buf: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([buf[at], buf[at + 1]])
}

fn rd_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

impl Ext4Reader {
    fn open(image: &Path, part_offset: u64) -> Result<Self, String> {
        let mut file = File::open(image).map_err(|e| format!("open {}: {e}", image.display()))?;
        let mut sb = [0u8; 1024];
        file.seek(SeekFrom::Start(part_offset + 1024))
            .map_err(|e| format!("seek superblock: {e}"))?;
        file.read_exact(&mut sb)
            .map_err(|e| format!("read superblock: {e}"))?;
        if rd_u16(&sb, 56) != EXT4_SUPER_MAGIC {
            return Err("not an ext4 filesystem (bad superblock magic)".to_string());
        }
        let block_size = 1024u64 << rd_u32(&sb, 24);
        let inode_size = rd_u16(&sb, 88) as u64;
        let feature_incompat = rd_u32(&sb, 96);
        let is_64bit = feature_incompat & INCOMPAT_64BIT != 0;
        let desc_size = if is_64bit {
            (rd_u16(&sb, 254) as u64).max(64)
        } else {
            32
        };
        Ok(Self {
            file,
            part_offset,
            sb: SuperBlock {
                block_size,
                inodes_per_group: rd_u32(&sb, 40),
                inode_size: if inode_size == 0 { 256 } else { inode_size },
                first_data_block: rd_u32(&sb, 20),
                desc_size,
                is_64bit,
            },
        })
    }

    fn read_at(&mut self, byte_offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; len];
        self.file
            .seek(SeekFrom::Start(self.part_offset + byte_offset))
            .map_err(|e| format!("seek {byte_offset}: {e}"))?;
        self.file
            .read_exact(&mut buf)
            .map_err(|e| format!("read {len}@{byte_offset}: {e}"))?;
        Ok(buf)
    }

    fn read_block(&mut self, block: u64) -> Result<Vec<u8>, String> {
        let bs = self.sb.block_size;
        self.read_at(block * bs, bs as usize)
    }

    /// Inode table block for the block group, from its group descriptor.
    fn inode_table_block(&mut self, group: u32) -> Result<u64, String> {
        // Group descriptor table starts in the block after the first data block.
        let gdt_block = self.sb.first_data_block as u64 + 1;
        let desc_off = gdt_block * self.sb.block_size + group as u64 * self.sb.desc_size;
        let desc = self.read_at(desc_off, self.sb.desc_size as usize)?;
        let lo = rd_u32(&desc, 8) as u64;
        let hi = if self.sb.is_64bit && self.sb.desc_size > 32 {
            rd_u32(&desc, 40) as u64
        } else {
            0
        };
        Ok(lo | (hi << 32))
    }

    /// Read a raw inode's bytes by number.
    fn read_inode(&mut self, ino: u32) -> Result<Vec<u8>, String> {
        let group = (ino - 1) / self.sb.inodes_per_group;
        let index = (ino - 1) % self.sb.inodes_per_group;
        let table = self.inode_table_block(group)?;
        let off = table * self.sb.block_size + index as u64 * self.sb.inode_size;
        self.read_at(off, self.sb.inode_size as usize)
    }

    /// Collect (physical_block, block_count) runs that map an extent-based inode's data,
    /// in logical order. Walks extent index nodes recursively for depth > 0.
    fn extent_runs(&mut self, node: &[u8]) -> Result<Vec<(u64, u64)>, String> {
        if rd_u16(node, 0) != EXT4_EXTENT_MAGIC {
            return Err("inode is not extent-mapped (legacy indirect blocks unsupported)".into());
        }
        let entries = rd_u16(node, 2) as usize;
        let depth = rd_u16(node, 6);
        let mut runs = Vec::new();
        for i in 0..entries {
            let e = 12 + i * 12;
            if depth == 0 {
                let len = rd_u16(node, e + 4) as u64;
                let start_hi = rd_u16(node, e + 6) as u64;
                let start_lo = rd_u32(node, e + 8) as u64;
                runs.push((start_lo | (start_hi << 32), len));
            } else {
                let leaf_lo = rd_u32(node, e + 4) as u64;
                let leaf_hi = rd_u16(node, e + 8) as u64;
                let child_block = leaf_lo | (leaf_hi << 32);
                let child = self.read_block(child_block)?;
                runs.extend(self.extent_runs(&child)?);
            }
        }
        Ok(runs)
    }

    /// Full data of a regular file inode, truncated to its size.
    fn read_file_data(&mut self, inode: &[u8]) -> Result<Vec<u8>, String> {
        let flags = rd_u32(inode, 32);
        if flags & EXT4_EXTENTS_FL == 0 {
            return Err("file inode is not extent-mapped (unsupported)".into());
        }
        let size = rd_u32(inode, 4) as u64 | ((rd_u32(inode, 108) as u64) << 32);
        let runs = self.extent_runs(&inode[40..40 + 60])?;
        let mut data = Vec::with_capacity(size as usize);
        for (start, len) in runs {
            for b in 0..len {
                data.extend_from_slice(&self.read_block(start + b)?);
            }
        }
        data.truncate(size as usize);
        Ok(data)
    }

    /// Find a child entry's inode number within a directory inode by name.
    fn lookup_in_dir(&mut self, dir_inode: &[u8], name: &str) -> Result<Option<u32>, String> {
        let flags = rd_u32(dir_inode, 32);
        if flags & EXT4_EXTENTS_FL == 0 {
            return Err("directory inode is not extent-mapped (unsupported)".into());
        }
        let runs = self.extent_runs(&dir_inode[40..40 + 60])?;
        for (start, len) in runs {
            for b in 0..len {
                let block = self.read_block(start + b)?;
                let mut pos = 0usize;
                while pos + 8 <= block.len() {
                    let child = rd_u32(&block, pos);
                    let rec_len = rd_u16(&block, pos + 4) as usize;
                    let name_len = block[pos + 6] as usize;
                    if rec_len == 0 {
                        break;
                    }
                    if child != 0 && name_len > 0 && pos + 8 + name_len <= block.len() {
                        let entry_name = &block[pos + 8..pos + 8 + name_len];
                        if entry_name == name.as_bytes() {
                            return Ok(Some(child));
                        }
                    }
                    pos += rec_len;
                }
            }
        }
        Ok(None)
    }
}

/// Extract the bytes of an absolute `/`-rooted file path from the ext4 filesystem at
/// `part_offset` inside `image`. Returns an error if any path component is missing or the
/// filesystem uses layouts this minimal reader does not support.
pub fn extract_file(image: &Path, part_offset: u64, path: &str) -> Result<Vec<u8>, String> {
    let mut reader = Ext4Reader::open(image, part_offset)?;
    let mut current = reader.read_inode(ROOT_INODE)?;
    let mut walked = String::new();
    for component in path.split('/').filter(|c| !c.is_empty()) {
        let child = reader
            .lookup_in_dir(&current, component)?
            .ok_or_else(|| format!("path component not found: {walked}/{component}"))?;
        current = reader.read_inode(child)?;
        walked.push('/');
        walked.push_str(component);
    }
    reader.read_file_data(&current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn base_image() -> Option<PathBuf> {
        let p = dirs_local_app_data()?.join("Pane/runtime/pane/images/arch-base.paneimg");
        p.exists().then_some(p)
    }

    fn dirs_local_app_data() -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }

    #[test]
    fn extracts_distro_initramfs_from_registered_base_image() {
        // Integration test: only runs when the registered base image is present.
        let Some(image) = base_image() else {
            eprintln!("skipping: base image not registered on this machine");
            return;
        };
        let data = extract_file(&image, 1_048_576, "/boot/initramfs-linux.img")
            .expect("extract initramfs");
        assert!(data.len() > 1_000_000, "initramfs too small: {}", data.len());
        // Arch initramfs is a (possibly compressed) cpio; first bytes are a known magic:
        // gzip 1f 8b, zstd 28 b5 2f fd, xz fd 37, lz4 04 22 4d 18, or raw newc cpio "0707".
        let magic_ok = data.starts_with(&[0x1f, 0x8b])
            || data.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
            || data.starts_with(&[0xfd, b'7', b'z'])
            || data.starts_with(&[0x04, 0x22, 0x4d, 0x18])
            || data.starts_with(b"0707");
        assert!(magic_ok, "unexpected initramfs magic: {:02x?}", &data[..4]);
    }

    #[test]
    fn extracts_distro_kernel_bzimage_from_registered_base_image() {
        // Validates the kernel auto-derive path: same reader pulls vmlinuz from the image.
        let Some(image) = base_image() else {
            eprintln!("skipping: base image not registered on this machine");
            return;
        };
        let data =
            extract_file(&image, 1_048_576, "/boot/vmlinuz-linux").expect("extract vmlinuz");
        assert!(data.len() > 1_000_000, "kernel too small: {}", data.len());
        // Linux bzImage carries the "HdrS" setup-header magic at offset 0x202.
        assert_eq!(&data[0x202..0x206], b"HdrS", "not a Linux bzImage");
    }
}
