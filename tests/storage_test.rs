use std::sync::Arc;
use std::thread;

use rust_ram_disk::engine::{CompressionMode, SparseMemoryStorage, StorageEngine};
use rust_ram_disk::persistence::ImagePersistence;

#[test]
fn test_sparse_allocation_and_read_zeros() {
    let capacity = 1024 * 1024 * 1024; // 1 GB virtual capacity
    let block_size = 4096;
    let storage = SparseMemoryStorage::new(capacity, block_size, CompressionMode::None);

    // Initial state: 0 physical memory used
    assert_eq!(storage.allocated_blocks(), 0);
    assert_eq!(storage.physical_memory_used(), 0);

    // Reading unwritten area should return all zeros
    let mut read_buf = vec![0xFFu8; 1024];
    let n = storage.read_at(100_000, &mut read_buf).unwrap();
    assert_eq!(n, 1024);
    assert!(read_buf.iter().all(|&b| b == 0));

    // Write 100 bytes at offset 500,000,000 (deep into the disk)
    let payload = b"Hello from Rust RAM Disk Sparse Allocator!";
    storage.write_at(500_000_000, payload).unwrap();

    // Exactly 1 block allocated!
    assert_eq!(storage.allocated_blocks(), 1);
    assert_eq!(storage.physical_memory_used(), block_size);

    // Read back payload
    let mut check_buf = vec![0u8; payload.len()];
    storage.read_at(500_000_000, &mut check_buf).unwrap();
    assert_eq!(&check_buf, payload);
}

#[test]
fn test_cross_block_boundary_read_write() {
    let capacity = 64 * 1024 * 1024; // 64 MB
    let block_size = 4096;
    let storage = SparseMemoryStorage::new(capacity, block_size, CompressionMode::None);

    // Offset at 4000, write 8000 bytes (spans across block 0, block 1, block 2)
    let pattern: Vec<u8> = (0..8000).map(|i| (i % 256) as u8).collect();
    let written = storage.write_at(4000, &pattern).unwrap();
    assert_eq!(written, 8000);

    assert_eq!(storage.allocated_blocks(), 3);

    let mut read_back = vec![0u8; 8000];
    let read = storage.read_at(4000, &mut read_back).unwrap();
    assert_eq!(read, 8000);
    assert_eq!(read_back, pattern);
}

#[test]
fn test_lz4_transparent_compression() {
    let capacity = 32 * 1024 * 1024;
    let block_size = 4096;
    let storage = SparseMemoryStorage::new(capacity, block_size, CompressionMode::Lz4);

    // Highly compressible payload: 16 KB of repeating text
    let text = b"Antigravity Rust RAM Disk High Performance Memory Storage! ";
    let mut payload = Vec::with_capacity(16384);
    while payload.len() < 16384 {
        payload.extend_from_slice(text);
    }
    payload.truncate(16384);

    storage.write_at(0, &payload).unwrap();

    // 4 blocks written
    assert_eq!(storage.allocated_blocks(), 4);

    // Verify LZ4 compression drastically reduces physical memory usage
    let physical_size = storage.physical_memory_used();
    assert!(
        physical_size < 1000,
        "Expected physical size to be < 1000 bytes due to LZ4 compression, got {}",
        physical_size
    );

    // Verify decompression read back is 100% faithful
    let mut read_back = vec![0u8; 16384];
    storage.read_at(0, &mut read_back).unwrap();
    assert_eq!(read_back, payload);
}

#[test]
fn test_snapshot_persistence() {
    let temp_dir = std::env::temp_dir();
    let img_path = temp_dir.join(format!("test_ramdisk_{}.img", std::process::id()));

    let capacity = 16 * 1024 * 1024;
    let storage = SparseMemoryStorage::new(capacity, 4096, CompressionMode::None);

    let sample_data = b"Persistent Snapshot Validation Payload 2026";
    storage.write_at(1024 * 1024, sample_data).unwrap();

    // Save snapshot
    ImagePersistence::save_sparse(&storage, &img_path).unwrap();

    // Load back into new storage instance
    let restored = ImagePersistence::load_sparse(&img_path, CompressionMode::None).unwrap();
    assert_eq!(restored.capacity(), capacity);
    assert_eq!(restored.allocated_blocks(), 1);

    let mut read_back = vec![0u8; sample_data.len()];
    restored.read_at(1024 * 1024, &mut read_back).unwrap();
    assert_eq!(&read_back, sample_data);

    // Clean up
    let _ = std::fs::remove_file(img_path);
}

#[test]
fn test_concurrent_read_write() {
    let capacity = 32 * 1024 * 1024;
    let storage = Arc::new(SparseMemoryStorage::new(capacity, 4096, CompressionMode::None));

    let mut handles = Vec::new();
    for thread_id in 0..8 {
        let storage_clone = Arc::clone(&storage);
        let handle = thread::spawn(move || {
            let offset = thread_id * 65536;
            let val = (thread_id as u8) + 1;
            let write_buf = vec![val; 4096];
            storage_clone.write_at(offset, &write_buf).unwrap();

            let mut read_buf = vec![0u8; 4096];
            storage_clone.read_at(offset, &mut read_buf).unwrap();
            assert_eq!(read_buf, write_buf);
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(storage.allocated_blocks(), 8);
}


#[test]
fn test_drive_snapshot_save_restore() {
    let temp_dir = std::env::temp_dir().join(format!("ramdisk_test_{}", std::process::id()));
    let source_dir = temp_dir.join("source");
    let dest_dir = temp_dir.join("dest");
    let snap_file = temp_dir.join("test_snapshot.img");

    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();

    // Create test files
    std::fs::write(source_dir.join("test1.txt"), b"Hello World").unwrap();
    std::fs::create_dir_all(source_dir.join("subdir")).unwrap();
    std::fs::write(source_dir.join("subdir").join("test2.bin"), vec![0xAB; 10000]).unwrap();

    // Save snapshot with LZ4 compression
    let saved = ImagePersistence::save_drive_snapshot(
        source_dir.to_str().unwrap(),
        &snap_file,
        CompressionMode::Lz4,
    ).unwrap();
    assert_eq!(saved, 2);

    // Restore snapshot to dest_dir
    let restored = ImagePersistence::restore_drive_snapshot(
        &snap_file,
        dest_dir.to_str().unwrap(),
    ).unwrap();
    assert_eq!(restored, 2);

    // Verify restored file contents
    let c1 = std::fs::read(dest_dir.join("test1.txt")).unwrap();
    assert_eq!(c1, b"Hello World");

    let c2 = std::fs::read(dest_dir.join("subdir").join("test2.bin")).unwrap();
    assert_eq!(c2, vec![0xAB; 10000]);

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_vfs_normalize_mount_point() {
    use rust_ram_disk::vfs::normalize_mount_point;
    assert_eq!(normalize_mount_point("s").unwrap(), "S:");
    assert_eq!(normalize_mount_point("S").unwrap(), "S:");
    assert_eq!(normalize_mount_point("S:").unwrap(), "S:");
    assert_eq!(normalize_mount_point("s:\\").unwrap(), "S:");
    assert_eq!(normalize_mount_point("r:/").unwrap(), "R:");
    assert!(normalize_mount_point("").is_err());
    assert!(normalize_mount_point("123").is_err());
}
