use super::{LineBasedRotation, LogReader, RecoveryLog};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinSet;

pub async fn test_multithread_logging() {
    println!("\n=== Test 1: Multi-threaded Logging ===");
    
    let log_path = PathBuf::from("./test_logs/multithread.log");
    let rotation = Arc::new(
        LineBasedRotation::new(log_path.clone(), 10, true)
            .expect("Failed to create rotation")
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
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        });
    }

    while let Some(result) = tasks.join_next().await {
        if let Err(e) = result {
            eprintln!("Task error: {}", e);
        }
    }

    rotation.flush().await.expect("Failed to flush");
    
    println!("✓ Successfully wrote {} messages from {} threads", 
             total_threads * messages_per_thread, total_threads);

    let files = rotation.list_log_files().expect("Failed to list files");
    println!("✓ Created {} log files (including compressed)", files.len());
    for file in &files {
        println!("  - {}", file.display());
    }
}

pub async fn test_log_rotation() {
    println!("\n=== Test 2: Log Rotation with Compression ===");
    
    let log_path = PathBuf::from("./test_logs/rotation.log");
    let max_lines = 10;
    let rotation = LineBasedRotation::new(log_path.clone(), max_lines, true)
        .expect("Failed to create rotation");

    let total_messages = 45;
    for i in 0..total_messages {
        let message = format!("Rotation test message #{}", i);
        rotation.write_line(&message).await.expect("Write failed");
    }

    rotation.flush().await.expect("Failed to flush");

    let files = rotation.list_log_files().expect("Failed to list files");
    println!("✓ Wrote {} messages with max {} lines per file", total_messages, max_lines);
    println!("✓ Created {} log files", files.len());

    let expected_files = (total_messages as f32 / max_lines as f32).ceil() as usize;
    assert!(files.len() >= expected_files, 
            "Expected at least {} files, got {}", expected_files, files.len());

    for file in &files {
        let metadata = std::fs::metadata(file).expect("Failed to get metadata");
        let file_type = if file.extension().and_then(|s| s.to_str()) == Some("gz") {
            "compressed"
        } else {
            "plain"
        };
        println!("  - {} ({}, {} bytes)", 
                 file.display(), file_type, metadata.len());
    }
}

pub async fn test_log_reading() {
    println!("\n=== Test 3: Log Reading ===");
    
    let log_dir = PathBuf::from("./test_logs");
    let reader = LogReader::new(&log_dir, "rotation")
        .expect("Failed to create reader");

    let files = reader.list_files();
    println!("✓ Found {} log files to read", files.len());

    let all_lines = reader.read_all_lines().expect("Failed to read lines");
    println!("✓ Read {} total lines from all files", all_lines.len());

    println!("  First 5 lines:");
    for (i, line) in all_lines.iter().take(5).enumerate() {
        println!("    {}: {}", i, line);
    }

    if all_lines.len() > 5 {
        println!("  ...");
        println!("  Last 3 lines:");
        for (i, line) in all_lines.iter().rev().take(3).rev().enumerate() {
            println!("    {}: {}", all_lines.len() - 3 + i, line);
        }
    }

    let line_count = reader.count_lines().expect("Failed to count lines");
    assert_eq!(line_count, all_lines.len(), "Line count mismatch");
    println!("✓ Verified line count: {}", line_count);
}

pub async fn test_recovery_log() {
    println!("\n=== Test 4: Recovery Log (WAL) ===");
    
    let log_path = PathBuf::from("./test_logs/recovery.wal");
    if log_path.exists() {
        std::fs::remove_file(&log_path).expect("Failed to clean old WAL");
    }

    let recovery_log = RecoveryLog::new(log_path.clone())
        .expect("Failed to create recovery log");

    println!("Writing transactions...");
    
    let tx1 = recovery_log.begin_transaction().await.expect("Failed to begin tx1");
    recovery_log.write_data(tx1, "user:1".to_string(), "Alice".to_string())
        .await.expect("Failed to write data");
    recovery_log.write_data(tx1, "user:2".to_string(), "Bob".to_string())
        .await.expect("Failed to write data");
    recovery_log.commit_transaction(tx1).await.expect("Failed to commit tx1");
    println!("✓ Transaction {} committed (2 writes)", tx1);

    let tx2 = recovery_log.begin_transaction().await.expect("Failed to begin tx2");
    recovery_log.write_data(tx2, "user:3".to_string(), "Charlie".to_string())
        .await.expect("Failed to write data");
    recovery_log.abort_transaction(tx2).await.expect("Failed to abort tx2");
    println!("✓ Transaction {} aborted (1 write discarded)", tx2);

    let tx3 = recovery_log.begin_transaction().await.expect("Failed to begin tx3");
    recovery_log.write_data(tx3, "user:1".to_string(), "Alice Updated".to_string())
        .await.expect("Failed to write data");
    recovery_log.commit_transaction(tx3).await.expect("Failed to commit tx3");
    println!("✓ Transaction {} committed (1 update)", tx3);

    recovery_log.checkpoint().await.expect("Failed to checkpoint");
    println!("✓ Checkpoint created");

    println!("\nRecovering from log...");
    let state = recovery_log.recover().expect("Failed to recover");
    
    println!("✓ Recovery completed:");
    println!("  - Committed data entries: {}", state.committed_data.len());
    println!("  - Active (uncommitted) transactions: {}", state.active_transactions.len());
    
    assert_eq!(state.get_value("user:1"), Some(&"Alice Updated".to_string()));
    assert_eq!(state.get_value("user:2"), Some(&"Bob".to_string()));
    assert_eq!(state.get_value("user:3"), None);
    
    println!("\n  Data state:");
    for (key, value) in &state.committed_data {
        println!("    {} = {}", key, value);
    }
}

pub async fn test_power_failure_recovery() {
    println!("\n=== Test 5: Power Failure Recovery ===");
    
    let log_path = PathBuf::from("./test_logs/power_failure.wal");
    if log_path.exists() {
        std::fs::remove_file(&log_path).expect("Failed to clean old WAL");
    }

    {
        println!("Simulating normal operations before power failure...");
        let recovery_log = RecoveryLog::new(log_path.clone())
            .expect("Failed to create recovery log");

        let tx1 = recovery_log.begin_transaction().await.expect("Failed to begin tx");
        recovery_log.write_data(tx1, "account:1".to_string(), "1000".to_string())
            .await.expect("Failed to write");
        recovery_log.write_data(tx1, "account:2".to_string(), "2000".to_string())
            .await.expect("Failed to write");
        recovery_log.commit_transaction(tx1).await.expect("Failed to commit");
        println!("✓ Committed: account:1=1000, account:2=2000");

        let tx2 = recovery_log.begin_transaction().await.expect("Failed to begin tx");
        recovery_log.write_data(tx2, "account:1".to_string(), "1500".to_string())
            .await.expect("Failed to write");
        recovery_log.write_data(tx2, "account:2".to_string(), "1500".to_string())
            .await.expect("Failed to write");
        println!("✓ Started transaction {} (transfer 500 from account:2 to account:1)", tx2);
        
        println!("💥 SIMULATING POWER FAILURE (transaction not committed)");
    }

    println!("\nSystem restarting... Recovering from WAL...");
    let recovery_log = RecoveryLog::new(log_path.clone())
        .expect("Failed to reopen recovery log");
    
    let state = recovery_log.recover().expect("Failed to recover");
    
    println!("✓ Recovery completed:");
    println!("  - Committed transactions: restored");
    println!("  - Uncommitted transactions: rolled back");
    println!("  - Active transactions at crash: {}", state.active_transactions.len());
    
    assert_eq!(state.get_value("account:1"), Some(&"1000".to_string()));
    assert_eq!(state.get_value("account:2"), Some(&"2000".to_string()));
    
    println!("\n  Recovered data state:");
    for (key, value) in &state.committed_data {
        println!("    {} = {}", key, value);
    }
    
    println!("\n✓ Data consistency verified: uncommitted transaction was rolled back");
}

pub async fn test_concurrent_recovery_writes() {
    println!("\n=== Test 6: Concurrent Recovery Log Writes ===");
    
    let log_path = PathBuf::from("./test_logs/concurrent.wal");
    if log_path.exists() {
        std::fs::remove_file(&log_path).expect("Failed to clean old WAL");
    }

    let recovery_log = Arc::new(
        RecoveryLog::new(log_path.clone()).expect("Failed to create recovery log")
    );

    let mut tasks = JoinSet::new();
    let num_threads = 3;
    let tx_per_thread = 5;

    for thread_id in 0..num_threads {
        let log_clone = recovery_log.clone();
        tasks.spawn(async move {
            for i in 0..tx_per_thread {
                let tx = log_clone.begin_transaction().await.expect("Begin failed");
                let key = format!("thread_{}:item_{}", thread_id, i);
                let value = format!("value_{}", tx);
                log_clone.write_data(tx, key, value).await.expect("Write failed");
                log_clone.commit_transaction(tx).await.expect("Commit failed");
                
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            }
        });
    }

    while let Some(result) = tasks.join_next().await {
        if let Err(e) = result {
            eprintln!("Task error: {}", e);
        }
    }

    println!("✓ {} threads completed {} transactions each", num_threads, tx_per_thread);

    let state = recovery_log.recover().expect("Failed to recover");
    let expected_entries = num_threads * tx_per_thread;
    
    println!("✓ Recovery found {} committed entries (expected {})", 
             state.committed_data.len(), expected_entries);
    assert_eq!(state.committed_data.len(), expected_entries);
    
    println!("✓ No uncommitted transactions: {}", !state.has_uncommitted());
    assert!(!state.has_uncommitted());
}

pub async fn run_all_tests() {
    println!("╔════════════════════════════════════════════════╗");
    println!("║   Logger System Comprehensive Test Suite     ║");
    println!("╚════════════════════════════════════════════════╝");

    std::fs::create_dir_all("./test_logs").expect("Failed to create test dir");

    test_multithread_logging().await;
    test_log_rotation().await;
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

