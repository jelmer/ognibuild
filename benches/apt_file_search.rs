use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ognibuild::debian::file_search::{
    get_apt_contents_file_searcher, setup_apt_file, FileSearcher, RemoteContentsFileSearcher,
};
use ognibuild::session::unshare::UnshareSession;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Benchmark data structure to hold different searcher implementations
struct BenchmarkData {
    remote_searcher: Option<RemoteContentsFileSearcher>,
    local_searcher: Option<RemoteContentsFileSearcher>,
    common_paths: Vec<&'static str>,
    common_regexes: Vec<&'static str>,
}

impl BenchmarkData {
    fn new() -> Self {
        println!("Loading real Debian Contents files for benchmarking...");

        // Create RemoteContents searcher - downloads from internet
        println!("Creating RemoteContents searcher (downloads from internet)...");
        let remote_searcher = match UnshareSession::bootstrap() {
            Ok(session) => {
                // Set up apt-file in the session
                match setup_apt_file(&session) {
                    Ok(_) => {
                        println!("APT setup completed successfully");
                        // Now get the file searcher
                        match get_apt_contents_file_searcher(&session) {
                            Ok(searcher) => {
                                println!("RemoteContents searcher created successfully");
                                // Convert Box<dyn FileSearcher> to RemoteContentsFileSearcher if possible
                                // For now, create a new one via from_session
                                match RemoteContentsFileSearcher::from_session(&session) {
                                    Ok(contents_searcher) => Some(contents_searcher),
                                    Err(e) => {
                                        println!("Warning: Failed to create RemoteContentsFileSearcher: {}", e);
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                println!("Warning: Failed to get file searcher: {}", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        println!("Warning: Failed to set up APT: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                println!(
                    "Warning: Failed to bootstrap UnshareSession for remote: {}",
                    e
                );
                None
            }
        };

        // Create LocalApt searcher - uses local APT cache
        println!("Creating LocalApt searcher (uses local /var/lib/apt/lists)...");
        let local_searcher = match UnshareSession::bootstrap() {
            Ok(session) => {
                // Create a temporary searcher to test local loading
                match RemoteContentsFileSearcher::from_session(&session) {
                    Ok(mut searcher) => {
                        // Try to reload from local cache instead
                        match searcher.load_local() {
                            Ok(_) => {
                                println!("LocalApt searcher created successfully");
                                Some(searcher)
                            }
                            Err(e) => {
                                println!("Warning: Failed to load from local APT cache: {}", e);
                                // Return the remote searcher as fallback
                                Some(searcher)
                            }
                        }
                    }
                    Err(e) => {
                        println!("Warning: Failed to create searcher for local APT: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                println!("Warning: Failed to bootstrap session for local APT: {}", e);
                None
            }
        };

        // Realistic file paths from actual Debian sid (these will match real entries)
        let common_paths = vec![
            // High-frequency exact matches (single results)
            "/usr/bin/python3.11",
            "/usr/bin/gcc-12",
            "/usr/bin/git",
            "/usr/bin/vim.basic",
            "/usr/bin/curl",
            "/usr/bin/make",
            "/usr/bin/node",
            // Common libraries (single results)
            "/lib/x86_64-linux-gnu/libc.so.6",
            "/usr/lib/x86_64-linux-gnu/libssl.so.3",
            "/usr/lib/x86_64-linux-gnu/libcurl.so.4",
            // Header files (single results)
            "/usr/include/stdio.h",
            "/usr/include/openssl/ssl.h",
            // Config files (single results)
            "/etc/hostname",
            "/etc/passwd",
            // Non-existent files (zero results)
            "/usr/bin/nonexistent-binary",
            "/etc/nonexistent.conf",
        ];

        // Realistic regex patterns with different result volumes based on actual Debian contents
        let common_regexes = vec![
            // High volume matches (thousands of results)
            r"^/usr/share/doc/",                   // ~50k+ matches
            r"^/usr/lib/python3\.\d+/",            // ~20k+ matches
            r"^/usr/share/locale/.*/LC_MESSAGES/", // ~10k+ matches
            // Medium volume matches (hundreds of results)
            r"^/usr/bin/[^/]*$",                     // ~5k matches
            r"^/usr/lib/x86_64-linux-gnu/lib.*\.so", // ~2k matches
            r"^/usr/include/.*\.h$",                 // ~15k matches
            // Low volume matches (tens of results)
            r"^/usr/bin/.*gcc.*",  // ~50 matches
            r"^/usr/bin/python.*", // ~20 matches
            r"^/etc/.*systemd.*",  // ~30 matches
            // Very specific matches (few results)
            r"firefox", // ~100 matches across all files
            r"vim.*",   // ~200 matches
            // Zero matches
            r"^/nonexistent/path/",
            r"totallyfakepattern123",
        ];

        println!("Benchmark setup complete");

        BenchmarkData {
            remote_searcher,
            local_searcher,
            common_paths,
            common_regexes,
        }
    }

    fn create_realistic_memory_db() -> HashMap<PathBuf, String> {
        // Create a realistic subset of data for memory searcher
        let mut db = HashMap::new();

        // Add common binaries
        db.insert(
            PathBuf::from("/usr/bin/python3.11"),
            "python3.11".to_string(),
        );
        db.insert(PathBuf::from("/usr/bin/gcc-12"), "gcc-12".to_string());
        db.insert(PathBuf::from("/usr/bin/git"), "git".to_string());
        db.insert(PathBuf::from("/usr/bin/vim.basic"), "vim".to_string());
        db.insert(PathBuf::from("/usr/bin/curl"), "curl".to_string());
        db.insert(PathBuf::from("/usr/bin/make"), "make".to_string());

        // Add some libraries
        db.insert(
            PathBuf::from("/lib/x86_64-linux-gnu/libc.so.6"),
            "libc6".to_string(),
        );
        db.insert(
            PathBuf::from("/usr/lib/x86_64-linux-gnu/libssl.so.3"),
            "libssl3".to_string(),
        );

        // Add some headers
        db.insert(
            PathBuf::from("/usr/include/stdio.h"),
            "libc6-dev".to_string(),
        );
        db.insert(
            PathBuf::from("/usr/include/openssl/ssl.h"),
            "libssl-dev".to_string(),
        );

        // Add config files
        db.insert(PathBuf::from("/etc/hostname"), "base-files".to_string());
        db.insert(PathBuf::from("/etc/passwd"), "base-passwd".to_string());

        // Add many more entries to simulate realistic volume
        for i in 0..1000 {
            db.insert(
                PathBuf::from(format!("/usr/share/doc/package{}/README", i)),
                format!("package{}", i),
            );
            db.insert(
                PathBuf::from(format!("/usr/lib/python3.11/site-packages/module{}.py", i)),
                "python3.11".to_string(),
            );
        }

        db
    }
}

fn bench_exact_path_searches(c: &mut Criterion) {
    let data = BenchmarkData::new();

    let mut group = c.benchmark_group("exact_path_search");
    group.measurement_time(Duration::from_secs(10));

    // Benchmark remote searcher if available
    if let Some(ref searcher) = data.remote_searcher {
        group.bench_with_input(
            BenchmarkId::new("single_path_search", "remote"),
            searcher,
            |b, searcher| {
                b.iter(|| {
                    for path in &data.common_paths {
                        let results: Vec<_> = searcher
                            .search_files(black_box(Path::new(path)), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("case_insensitive_search", "remote"),
            searcher,
            |b, searcher| {
                b.iter(|| {
                    for path in &data.common_paths {
                        let results: Vec<_> = searcher
                            .search_files(black_box(Path::new(path)), true)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );
    }

    // Benchmark local searcher if available
    if let Some(ref searcher) = data.local_searcher {
        group.bench_with_input(
            BenchmarkId::new("single_path_search", "local"),
            searcher,
            |b, searcher| {
                b.iter(|| {
                    for path in &data.common_paths {
                        let results: Vec<_> = searcher
                            .search_files(black_box(Path::new(path)), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("case_insensitive_search", "local"),
            searcher,
            |b, searcher| {
                b.iter(|| {
                    for path in &data.common_paths {
                        let results: Vec<_> = searcher
                            .search_files(black_box(Path::new(path)), true)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_regex_searches(c: &mut Criterion) {
    let data = BenchmarkData::new();

    let mut group = c.benchmark_group("regex_search");
    group.measurement_time(Duration::from_secs(20));

    let high_volume_patterns = &data.common_regexes[0..3]; // First 3 are high volume
    let medium_volume_patterns = &data.common_regexes[3..6]; // Next 3 are medium volume
    let low_volume_patterns = &data.common_regexes[6..9]; // Next 3 are low volume
    let _zero_match_patterns = &data.common_regexes[9..]; // Last ones have zero matches

    // Benchmark remote searcher if available
    if let Some(ref searcher) = data.remote_searcher {
        // High volume regex searches (thousands of results)
        group.bench_with_input(
            BenchmarkId::new("regex_high_volume", "remote"),
            searcher,
            |b, searcher| {
                b.iter(|| {
                    for pattern in high_volume_patterns {
                        let results: Vec<_> = searcher
                            .search_files_regex(black_box(pattern), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );

        // Medium volume regex searches
        group.bench_with_input(
            BenchmarkId::new("regex_medium_volume", "remote"),
            searcher,
            |b, searcher| {
                b.iter(|| {
                    for pattern in medium_volume_patterns {
                        let results: Vec<_> = searcher
                            .search_files_regex(black_box(pattern), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );

        // Low volume regex searches
        group.bench_with_input(
            BenchmarkId::new("regex_low_volume", "remote"),
            searcher,
            |b, searcher| {
                b.iter(|| {
                    for pattern in low_volume_patterns {
                        let results: Vec<_> = searcher
                            .search_files_regex(black_box(pattern), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );
    }

    // Benchmark local searcher if available
    if let Some(ref searcher) = data.local_searcher {
        group.bench_with_input(
            BenchmarkId::new("regex_high_volume", "local"),
            searcher,
            |b, searcher| {
                b.iter(|| {
                    for pattern in high_volume_patterns {
                        let results: Vec<_> = searcher
                            .search_files_regex(black_box(pattern), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_repeated_queries(c: &mut Criterion) {
    let data = BenchmarkData::new();

    let mut group = c.benchmark_group("repeated_queries");
    group.measurement_time(Duration::from_secs(15));

    // Benchmark remote searcher if available
    if let Some(ref searcher) = data.remote_searcher {
        // Benchmark repeated exact queries (should show caching benefits)
        group.bench_with_input(
            BenchmarkId::new("repeated_exact_queries", "remote"),
            searcher,
            |b, searcher| {
                let test_path = "/usr/bin/python3.11";
                b.iter(|| {
                    for _ in 0..100 {
                        let results: Vec<_> = searcher
                            .search_files(black_box(Path::new(test_path)), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );

        // Benchmark repeated regex queries (tests regex caching)
        group.bench_with_input(
            BenchmarkId::new("repeated_regex_queries", "remote"),
            searcher,
            |b, searcher| {
                let test_pattern = r"^/usr/bin/.*gcc.*";
                b.iter(|| {
                    for _ in 0..50 {
                        let results: Vec<_> = searcher
                            .search_files_regex(black_box(test_pattern), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );
    }

    // Benchmark local searcher if available
    if let Some(ref searcher) = data.local_searcher {
        group.bench_with_input(
            BenchmarkId::new("repeated_exact_queries", "local"),
            searcher,
            |b, searcher| {
                let test_path = "/usr/bin/python3.11";
                b.iter(|| {
                    for _ in 0..100 {
                        let results: Vec<_> = searcher
                            .search_files(black_box(Path::new(test_path)), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("repeated_regex_queries", "local"),
            searcher,
            |b, searcher| {
                let test_pattern = r"^/usr/bin/.*gcc.*";
                b.iter(|| {
                    for _ in 0..50 {
                        let results: Vec<_> = searcher
                            .search_files_regex(black_box(test_pattern), false)
                            .collect();
                        black_box(results);
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_exact_path_searches,
    bench_regex_searches,
    bench_repeated_queries,
);
criterion_main!(benches);
