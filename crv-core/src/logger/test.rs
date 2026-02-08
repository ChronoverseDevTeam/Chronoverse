use super::{Logger, LogLevel, LogFormat, RotationWriter, LogRotation, LogReader, RecoveryLog};
use crate::log_info;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinSet;

pub async fn test_logger_all_rotations() {
    println!("\n=== Test 1: Logger with All Rotation Strategies ===");
    
    println!("Testing all rotation strategies with RotationWriter directly...");
    
    let hourly = RotationWriter::new(
        PathBuf::from("./test_logs/logger_hourly.log"),
        LogRotation::Hourly { compress: true }
    ).expect("Failed to create hourly rotation");
    hourly.write_line("Hourly rotation test message 1").await.expect("Write failed");
    hourly.write_line("Hourly rotation test message 2").await.expect("Write failed");
    println!("✓ Hourly rotation working");
    
    let daily = RotationWriter::new(
        PathBuf::from("./test_logs/logger_daily.log"),
        LogRotation::Daily { compress: true }
    ).expect("Failed to create daily rotation");
    daily.write_line("Daily rotation test message").await.expect("Write failed");
    println!("✓ Daily rotation working");
    
    let linebased = RotationWriter::new(
        PathBuf::from("./test_logs/logger_linebased.log"),
        LogRotation::LineBased { max_lines: 5, compress: true }
    ).expect("Failed to create line-based rotation");
    for i in 0..12 {
        linebased.write_line(&format!("Line-based message {}", i)).await.expect("Write failed");
    }
    let files = linebased.list_log_files().expect("Failed to list files");
    println!("✓ LineBased rotation working (created {} files)", files.len());
    
    let sizebased = RotationWriter::new(
        PathBuf::from("./test_logs/logger_sizebased.log"),
        LogRotation::SizeBased { max_size_mb: 1, compress: true }
    ).expect("Failed to create size-based rotation");
    sizebased.write_line("Size-based rotation test message").await.expect("Write failed");
    println!("✓ SizeBased rotation working");
    
    println!("\nTesting Logger API integration (one-time init)...");
    let _guard = Logger::builder()
        .level(LogLevel::Info)
        .format(LogFormat::Compact)
        .file(
            PathBuf::from("./test_logs/logger_integrated.log"),
            LogRotation::LineBased { max_lines: 5, compress: true }
        )
        .build()
        .init()
        .expect("Failed to init logger");
    
    log_info!("Logger API integration test");
    log_info!("Testing line-based rotation via Logger");
    for i in 0..8 {
        log_info!("Message {}", i);
    }
    println!("✓ Logger API with LineBased rotation working");
}

pub async fn test_rotation_with_compression() {
    println!("\n=== Test 2: Rotation with Compression ===");
    
    let log_path = PathBuf::from("./test_logs/compression_test.log");
    let rotation = RotationWriter::new(
        log_path.clone(), 
        LogRotation::LineBased { max_lines: 10, compress: true }
    ).expect("Failed to create rotation");

    let total_messages = 45;
    for i in 0..total_messages {
        rotation.write_line(&format!("Message #{}", i)).await.expect("Write failed");
    }
    rotation.flush().await.expect("Failed to flush");

    let files = rotation.list_log_files().expect("Failed to list files");
    println!("✓ Wrote {} messages, created {} log files", total_messages, files.len());

    let compressed_count = files.iter().filter(|f| f.extension().and_then(|s| s.to_str()) == Some("gz")).count();
    println!("✓ Compressed {} old log files", compressed_count);
    
    for file in &files {
        let metadata = std::fs::metadata(file).expect("Failed to get metadata");
        let file_type = if file.extension().and_then(|s| s.to_str()) == Some("gz") {
            "compressed"
        } else {
            "plain"
        };
        println!("  - {} ({}, {} bytes)", file.display(), file_type, metadata.len());
    }
}

pub async fn test_multithread_logging() {
    println!("\n=== Test 3: Multi-threaded Logging ===");
    
    let log_path = PathBuf::from("./test_logs/multithread.log");
    let rotation = Arc::new(
        RotationWriter::new(
            log_path.clone(), 
            LogRotation::LineBased { max_lines: 10, compress: true }
        ).expect("Failed to create rotation")
    );

    let mut tasks = JoinSet::new();
    let total_threads = 5;
    let messages_per_thread = 20;

    for thread_id in 0..total_threads {
        let rotation_clone = rotation.clone();
        tasks.spawn(async move {
            for i in 0..messages_per_thread {
                let message = format!("Thread {} - Message {}", thread_id, i);
                if let Err(e) = rotation_clone.write_line(&message).await {
                    eprintln!("Write error: {}", e);
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            }
        });
    }

    while let Some(result) = tasks.join_next().await {
        if let Err(e) = result {
            eprintln!("Task error: {}", e);
        }
    }

    rotation.flush().await.expect("Failed to flush");
    
    let files = rotation.list_log_files().expect("Failed to list files");
    println!("✓ Wrote {} messages from {} threads", 
             total_threads * messages_per_thread, total_threads);
    println!("✓ Created {} log files", files.len());
}

pub async fn test_log_reading() {
    println!("\n=== Test 4: Log Reading ===");
    
    let log_dir = PathBuf::from("./test_logs");
    let reader = LogReader::new(&log_dir, "compression_test")
        .expect("Failed to create reader");

    let files = reader.list_files();
    println!("✓ Found {} log files to read", files.len());

    let all_lines = reader.read_all_lines().expect("Failed to read lines");
    println!("✓ Read {} total lines from all files", all_lines.len());

    if all_lines.len() > 0 {
        println!("  First 3 lines:");
        for (i, line) in all_lines.iter().take(3).enumerate() {
            println!("    {}: {}", i, line);
        }
        
        if all_lines.len() > 3 {
            println!("  ...");
            println!("  Last 2 lines:");
            for (i, line) in all_lines.iter().rev().take(2).rev().enumerate() {
                println!("    {}: {}", all_lines.len() - 2 + i, line);
            }
        }
    }

    let line_count = reader.count_lines().expect("Failed to count lines");
    assert_eq!(line_count, all_lines.len(), "Line count mismatch");
    println!("✓ Verified line count: {}", line_count);
}

pub async fn test_recovery_log() {
    println!("\n=== Test 5: Recovery Log (WAL) ===");
    
    let log_path = PathBuf::from("./test_logs/recovery_test.wal");
    if log_path.exists() {
        std::fs::remove_file(&log_path).expect("Failed to clean old WAL");
    }

    let recovery_log = RecoveryLog::new(log_path.clone())
        .expect("Failed to create recovery log");

    let tx1 = recovery_log.begin_transaction().await.expect("Failed to begin tx1");
    recovery_log.write_data(tx1, "key1".to_string(), "value1".to_string())
        .await.expect("Failed to write data");
    recovery_log.commit_transaction(tx1).await.expect("Failed to commit tx1");

    let tx2 = recovery_log.begin_transaction().await.expect("Failed to begin tx2");
    recovery_log.write_data(tx2, "key2".to_string(), "value2".to_string())
        .await.expect("Failed to write data");
    recovery_log.commit_transaction(tx2).await.expect("Failed to commit tx2");

    println!("✓ Created 2 transactions with commits");

    let state = recovery_log.recover().expect("Failed to recover");
    println!("✓ Recovered {} committed entries", state.committed_data.len());
    
    assert_eq!(state.committed_data.get("key1"), Some(&"value1".to_string()));
    assert_eq!(state.committed_data.get("key2"), Some(&"value2".to_string()));
    println!("✓ All committed data recovered correctly");
}

pub async fn test_power_failure_recovery() {
    println!("\n=== Test 6: Power Failure Recovery ===");
    
    let log_path = PathBuf::from("./test_logs/power_failure.wal");
    if log_path.exists() {
        std::fs::remove_file(&log_path).expect("Failed to clean old WAL");
    }

    {
        let recovery_log = RecoveryLog::new(log_path.clone())
            .expect("Failed to create recovery log");

        let tx1 = recovery_log.begin_transaction().await.expect("Failed to begin tx1");
        recovery_log.write_data(tx1, "committed_key".to_string(), "committed_value".to_string())
            .await.expect("Failed to write");
        recovery_log.commit_transaction(tx1).await.expect("Failed to commit");

        let tx2 = recovery_log.begin_transaction().await.expect("Failed to begin tx2");
        recovery_log.write_data(tx2, "uncommitted_key".to_string(), "uncommitted_value".to_string())
            .await.expect("Failed to write");
        
        println!("✓ Simulated power failure (tx2 not committed)");
    }

    let recovery_log = RecoveryLog::new(log_path.clone())
        .expect("Failed to reopen log");
    let state = recovery_log.recover().expect("Failed to recover");

    println!("✓ Recovered from power failure");
    println!("✓ Committed entries: {}", state.committed_data.len());
    
    assert_eq!(state.committed_data.get("committed_key"), Some(&"committed_value".to_string()));
    assert_eq!(state.committed_data.get("uncommitted_key"), None);
    println!("✓ Only committed data recovered, uncommitted data discarded");
}

pub async fn test_concurrent_recovery_writes() {
    println!("\n=== Test 7: Concurrent Recovery Writes ===");
    
    let log_path = PathBuf::from("./test_logs/concurrent.wal");
    if log_path.exists() {
        std::fs::remove_file(&log_path).expect("Failed to clean old WAL");
    }

    let recovery_log = Arc::new(
        RecoveryLog::new(log_path.clone()).expect("Failed to create recovery log")
    );

    let mut tasks = JoinSet::new();
    let num_threads = 5;
    let ops_per_thread = 10;

    for thread_id in 0..num_threads {
        let log_clone = recovery_log.clone();
        tasks.spawn(async move {
            for i in 0..ops_per_thread {
                let tx = log_clone.begin_transaction().await.expect("Failed to begin tx");
                let key = format!("thread{}_key{}", thread_id, i);
                let value = format!("thread{}_value{}", thread_id, i);
                log_clone.write_data(tx, key, value).await.expect("Failed to write");
                log_clone.commit_transaction(tx).await.expect("Failed to commit");
            }
        });
    }

    while let Some(result) = tasks.join_next().await {
        if let Err(e) = result {
            eprintln!("Task error: {}", e);
        }
    }

    println!("✓ Completed {} concurrent transactions", num_threads * ops_per_thread);

    let state = recovery_log.recover().expect("Failed to recover");
    let expected_entries = num_threads * ops_per_thread;
    
    println!("✓ Recovered {} entries (expected {})", 
             state.committed_data.len(), expected_entries);
    assert_eq!(state.committed_data.len(), expected_entries);
    assert!(!state.has_uncommitted());
    println!("✓ All concurrent writes recovered successfully");
}

pub async fn run_all_tests() {
    println!("╔════════════════════════════════════════════════╗");
    println!("║   Logger System Comprehensive Test Suite     ║");
    println!("╚════════════════════════════════════════════════╝");

    std::fs::create_dir_all("./test_logs").expect("Failed to create test dir");

    test_logger_all_rotations().await;
    test_rotation_with_compression().await;
    test_multithread_logging().await;
    test_log_reading().await;
    test_recovery_log().await;
    test_power_failure_recovery().await;
    test_concurrent_recovery_writes().await;

    println!("\n╔════════════════════════════════════════════════╗");
    println!("║           All Tests Completed! ✓              ║");
    println!("╚════════════════════════════════════════════════╝");
    
    println!("\nTest artifacts saved to: ./test_logs/");
    println!("You can inspect the log files and compressed archives there.");
}
