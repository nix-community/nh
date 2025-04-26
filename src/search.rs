use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Instant, SystemTime};

use color_eyre::eyre::{bail, Context, Result};
use elasticsearch_dsl::{Operator, Query, Search, SearchResponse, TextQueryType};
use interface::SearchArgs;
use once_cell::sync::Lazy;
use owo_colors::OwoColorize;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace, warn};

use crate::interface;

const DEPRECATED_VERSIONS: &[&str] = &["nixos-24.05"];
const DEFAULT_CACHE_DURATION: u64 = 3600; // 1 hour in seconds
const CACHE_DIR: &str = ".cache/nh";
const CACHE_FILE_EXT: &str = "json";

static NIXOS_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"nixos-[0-9]+\.[0-9]+").expect("Failed to compile regex"));

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(non_snake_case, dead_code)]
struct SearchResult {
    // r#type: String,
    package_attr_name: String,
    package_attr_set: String,
    package_pname: String,
    package_pversion: String,
    package_platforms: Vec<String>,
    package_outputs: Vec<String>,
    package_default_output: Option<String>,
    package_programs: Vec<String>,
    // package_license: Vec<License>,
    package_license_set: Vec<String>,
    // package_maintainers: Vec<HashMap<String, String>>,
    package_description: Option<String>,
    package_longDescription: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_hydra: Option<()>,
    package_system: String,
    package_homepage: Vec<String>,
    package_position: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedResults {
    timestamp: SystemTime,
    channel: String,
    query: Vec<String>,
    results: Vec<SearchResult>,
}

/// Returns the user's home directory or fallbacks to '/tmp' if not available.
fn get_home_dir() -> PathBuf {
    env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

impl SearchArgs {
    pub fn run(&self) -> Result<()> {
        trace!("args: {self:?}");

        if !supported_branch(&self.channel) {
            bail!("Channel {} is not supported!", self.channel);
        }

        // Start nixpkgs path lookup immediately
        let nixpkgs_path_handle = std::thread::spawn(|| {
            std::process::Command::new("nix")
                .stderr(Stdio::null())
                .args(["eval", "--raw", "-f", "<nixpkgs>", "path"])
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

        // Check for cached results if caching is enabled
        if self.use_cache {
            if let Some(results) = self.get_cached_results()? {
                self.display_results(results)?;
                return Ok(());
            }
        }

        let query_s = self.query.join(" ");
        debug!(?query_s);

        let client = reqwest::blocking::Client::builder()
            .user_agent(format!("nh/{}", crate::NH_VERSION))
            .timeout(std::time::Duration::from_secs(15))
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build reqwest client")?;

        println!(
            "Querying search.nixos.org, with channel {}...",
            self.channel
        );
        let then = Instant::now();

        let documents = match self.perform_elasticsearch_search(&client, &query_s) {
            Ok(docs) => docs,
            Err(e) => {
                warn!("Primary search failed: {}", e);
                if self.use_fallback
                    && (self.fallback_file.is_some() || !self.fallback_endpoints.is_empty())
                {
                    match self.try_fallback_methods(&client, &query_s) {
                        Ok(Some(docs)) => docs,
                        Ok(None) => bail!("Fallback methods failed to find results"),
                        Err(fallback_err) => {
                            bail!(
                                "Primary search failed: {}\nFallback failed: {}",
                                e,
                                fallback_err
                            )
                        }
                    }
                } else {
                    bail!(
                        "Primary search failed and no fallback methods available: {}",
                        e
                    );
                }
            }
        };

        let elapsed = then.elapsed();
        debug!(?elapsed);

        println!("Took {}ms", elapsed.as_millis());
        if !documents.is_empty() {
            println!("Most relevant results at the end");
            println!();
        } else {
            println!("No results found.");
            return Ok(());
        }

        // Cache the results in background thread if enabled
        if self.use_cache {
            let docs_clone = documents.clone();
            // Extract only the needed fields for caching
            let query = self.query.clone();
            let channel = self.channel.clone();
            let limit = self.limit;
            let cache_duration = self.cache_duration;

            std::thread::spawn(move || {
                cache_results_in_background(query, channel, limit, cache_duration, docs_clone);
            });
        }

        let nixpkgs_path = nixpkgs_path_handle
            .join()
            .unwrap_or(None)
            .unwrap_or_default();

        self.display_results_with_nixpkgs(documents, &nixpkgs_path)?;

        Ok(())
    }

    /// Perform the primary search query against Elasticsearch endpoint
    fn perform_elasticsearch_search(
        &self,
        client: &reqwest::blocking::Client,
        query_s: &str,
    ) -> Result<Vec<SearchResult>> {
        let backend_version: i8 = 42;

        // Pre-build the search query
        let query = Search::new().from(0).size(self.limit).query(
            Query::bool().filter(Query::term("type", "package")).must(
                Query::dis_max()
                    .tie_breaker(0.7)
                    .query(
                        Query::multi_match(
                            [
                                "package_attr_name^9",
                                "package_attr_name.*^5.3999999999999995",
                                "package_programs^9",
                                "package_programs.*^5.3999999999999995",
                                "package_pname^6",
                                "package_pname.*^3.5999999999999996",
                                "package_description^1.3",
                                "package_description.*^0.78",
                                "package_longDescription^1",
                                "package_longDescription.*^0.6",
                                "flake_name^0.5",
                                "flake_name.*^0.3",
                            ],
                            query_s,
                        )
                        .r#type(TextQueryType::CrossFields)
                        .analyzer("whitespace")
                        .auto_generate_synonyms_phrase_query(false)
                        .operator(Operator::And),
                    )
                    .query(
                        Query::wildcard("package_attr_name", format!("*{query_s}*"))
                            .case_insensitive(true),
                    ),
            ),
        );

        // Optimize request construction
        let req = client
            .post(format!(
                "https://search.nixos.org/backend/latest-{}-{}/_search",
                backend_version, self.channel
            ))
            .json(&query)
            .basic_auth("aWVSALXpZv", Some("X8gPHnzL52wFEekuxsfQ9cSh"))
            .build()
            .context("building search query")?;

        debug!(?req);

        // Execute request
        let response = client
            .execute(req)
            .context("querying the elasticsearch API")?;

        trace!(?response);

        if !response.status().is_success() {
            bail!(
                "Elasticsearch query failed with status: {}",
                response.status()
            );
        }

        // Parse response directly with json() for better performance
        let parsed_response: SearchResponse = response
            .json()
            .context("parsing response from elasticsearch")?;

        trace!(?parsed_response);

        parsed_response
            .documents::<SearchResult>()
            .context("parsing search document")
    }

    /// Attempts multiple fallback search methods in parallel when primary search fails.
    fn try_fallback_methods(
        &self,
        client: &reqwest::blocking::Client,
        query_s: &str,
    ) -> Result<Option<Vec<SearchResult>>> {
        // Try multiple fallback methods in parallel
        let mut handles = Vec::new();

        // Add file fallback if configured
        if let Some(fallback_file) = &self.fallback_file {
            let path = fallback_file.clone();
            let query = query_s.to_string();
            let limit = self.limit;
            let cache_duration = self.cache_duration;
            let channel = self.channel.clone();
            handles.push(std::thread::spawn(move || {
                let mut args = Self {
                    query: vec![query.clone()],
                    channel,
                    limit,
                    cache_duration,
                    platforms: false,
                    use_cache: false,
                    use_fallback: false,
                    fallback_file: None,
                    fallback_endpoints: Vec::new(),
                };
                args.fallback_file = Some(path.clone());
                args.try_file_fallback(&path, &query)
            }));
        }

        // Add endpoint fallbacks in parallel
        for endpoint in &self.fallback_endpoints {
            let endpoint_str = endpoint.clone();
            let query = query_s.to_string();
            let channel = self.channel.clone();
            let client = client.clone();
            let limit = self.limit;
            let cache_duration = self.cache_duration;
            handles.push(std::thread::spawn(move || {
                let args = Self {
                    query: vec![],
                    channel,
                    limit,
                    cache_duration,
                    platforms: false,
                    use_cache: false,
                    use_fallback: false,
                    fallback_file: None,
                    fallback_endpoints: Vec::new(),
                };
                args.try_endpoint_fallback(&client, &endpoint_str, &query)
            }));
        }

        // Check results from all threads
        for handle in handles {
            let Ok(result) = handle.join().expect("Thread panicked") else {
                continue;
            };
            if let Some(docs) = result {
                if !docs.is_empty() {
                    return Ok(Some(docs));
                }
            }
        }

        Ok(None)
    }

    fn try_file_fallback(
        &self,
        path: &PathBuf,
        query_s: &str,
    ) -> Result<Option<Vec<SearchResult>>> {
        if !path.exists() {
            return Ok(None);
        }

        // Read file directly into memory
        let mut file = File::open(path).context("opening fallback file")?;
        let mut content = Vec::with_capacity(
            file.metadata()
                .map(|m| m.len() as usize)
                .unwrap_or(1024 * 1024),
        );
        file.read_to_end(&mut content)
            .context("reading fallback file")?;

        let mut packages: Vec<SearchResult> =
            serde_json::from_slice(&content).context("parsing fallback file with serde_json")?;

        // Filter by query using lowercase for case-insensitive matching
        let query_lower = query_s.to_lowercase();

        packages.retain(|pkg| {
            pkg.package_attr_name.to_lowercase().contains(&query_lower)
                || pkg.package_pname.to_lowercase().contains(&query_lower)
                || pkg
                    .package_description
                    .as_ref()
                    .map_or(false, |d| d.to_lowercase().contains(&query_lower))
                || pkg
                    .package_longDescription
                    .as_ref()
                    .map_or(false, |d| d.to_lowercase().contains(&query_lower))
                || pkg
                    .package_programs
                    .iter()
                    .any(|p| p.to_lowercase().contains(&query_lower))
        });

        // Sort by relevance - prioritize exact matches in attr_name
        packages.sort_unstable_by(|a, b| {
            let a_exact = a.package_attr_name.to_lowercase() == query_lower;
            let b_exact = b.package_attr_name.to_lowercase() == query_lower;

            if a_exact != b_exact {
                return b_exact.cmp(&a_exact);
            }

            let a_contains = a.package_attr_name.to_lowercase().contains(&query_lower);
            let b_contains = b.package_attr_name.to_lowercase().contains(&query_lower);

            b_contains.cmp(&a_contains)
        });

        // Limit results
        if packages.len() > self.limit as usize {
            packages.truncate(self.limit as usize);
        }

        if packages.is_empty() {
            Ok(None)
        } else {
            Ok(Some(packages))
        }
    }

    fn try_endpoint_fallback(
        &self,
        client: &reqwest::blocking::Client,
        endpoint: &str,
        query_s: &str,
    ) -> Result<Option<Vec<SearchResult>>> {
        let request_url = endpoint
            .replace("{channel}", &self.channel)
            .replace("{query}", query_s);

        debug!("Trying fallback endpoint: {}", request_url);

        let response = client
            .get(&request_url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .context(format!("sending request to fallback endpoint: {endpoint}"))?;

        if response.status().is_success() {
            match response.json::<Vec<SearchResult>>() {
                Ok(mut packages) => {
                    if packages.len() > self.limit as usize {
                        packages.truncate(self.limit as usize);
                    }
                    if !packages.is_empty() {
                        Ok(Some(packages))
                    } else {
                        Ok(None)
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from endpoint {}: {}", endpoint, e);
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    fn get_cache_path(&self) -> Result<PathBuf> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let query_hash = {
            let mut hasher = DefaultHasher::new();
            self.query.hash(&mut hasher);
            self.channel.hash(&mut hasher);
            self.limit.hash(&mut hasher);
            hasher.finish()
        };

        let cache_dir = get_home_dir().join(CACHE_DIR);

        // Create directory only if needed
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir).context("creating cache directory")?;
        }

        Ok(cache_dir.join(format!(
            "search_{}_{}.{}",
            self.channel, query_hash, CACHE_FILE_EXT
        )))
    }

    fn get_cached_results(&self) -> Result<Option<Vec<SearchResult>>> {
        let cache_path = match self.get_cache_path() {
            Ok(path) => path,
            Err(e) => {
                warn!("Failed to get cache path: {}", e);
                return Ok(None);
            }
        };

        if !cache_path.exists() {
            return Ok(None);
        }

        // Read file directly into memory for better performance
        let mut file = match File::open(&cache_path) {
            Ok(file) => file,
            Err(e) => {
                warn!("Failed to open cache file {}: {}", cache_path.display(), e);
                let _ = fs::remove_file(&cache_path);
                return Ok(None);
            }
        };

        let metadata = file.metadata().ok();
        let file_size = metadata.as_ref().map_or(0, |m| m.len() as usize);
        let mut content = Vec::with_capacity(file_size.max(1024));

        if let Err(e) = file.read_to_end(&mut content) {
            warn!("Failed to read cache file {}: {}", cache_path.display(), e);
            let _ = fs::remove_file(&cache_path);
            return Ok(None);
        }

        let cached: CachedResults = match serde_json::from_slice(&content) {
            Ok(cached) => cached,
            Err(e) => {
                warn!("Failed to parse cache: {}, removing file", e);
                let _ = fs::remove_file(&cache_path);
                return Ok(None);
            }
        };

        // Check if cache is still valid
        let cache_duration = self.cache_duration.unwrap_or(DEFAULT_CACHE_DURATION);
        match cached.timestamp.elapsed() {
            Ok(elapsed) if elapsed.as_secs() <= cache_duration => {
                info!("Using cached results from {}s ago", elapsed.as_secs());
                Ok(Some(cached.results))
            }
            Ok(_) => {
                // Remove expired cache file in background
                // "Cache invalidation and naming things"
                let path = cache_path;
                std::thread::spawn(move || {
                    let _ = fs::remove_file(path);
                });
                Ok(None)
            }
            Err(_) => Ok(None),
        }
    }

    fn display_results(&self, results: Vec<SearchResult>) -> Result<()> {
        // Get nixpkgs path without blocking for too long
        let nixpkgs_path = std::thread::spawn(|| {
            let output = std::process::Command::new("nix")
                .stderr(Stdio::null())
                .args(["eval", "--raw", "-f", "<nixpkgs>", "path"])
                .output();

            match output {
                Ok(output) if output.status.success() => String::from_utf8(output.stdout)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                _ => String::new(),
            }
        })
        .join()
        .unwrap_or_else(|_| String::new());

        self.display_results_with_nixpkgs(results, &nixpkgs_path)
    }

    fn display_results_with_nixpkgs(
        &self,
        results: Vec<SearchResult>,
        nixpkgs_path: &str,
    ) -> Result<()> {
        if results.is_empty() {
            println!("No results found.");
            return Ok(());
        }

        let hyperlinks = supports_hyperlinks::supports_hyperlinks();
        debug!(?hyperlinks);
        let term_width = textwrap::termwidth();

        // Use larger buffer for stdout
        let stdout = std::io::stdout();
        let mut writer = BufWriter::with_capacity(65536, stdout.lock());

        for elem in results.iter().rev() {
            writeln!(writer)?;

            write!(writer, "{}", elem.package_attr_name.blue())?;
            let v = &elem.package_pversion;
            if !v.is_empty() {
                write!(writer, " ({})", v.green())?;
            }
            writeln!(writer)?;

            if let Some(ref desc) = elem.package_description {
                let desc = desc.replace('\n', " ");
                if desc.len() < term_width - 2 {
                    writeln!(writer, "  {desc}")?;
                } else {
                    let wrap_options = textwrap::Options::new(term_width)
                        .initial_indent("  ")
                        .subsequent_indent("  ");
                    for line in textwrap::wrap(&desc, wrap_options) {
                        writeln!(writer, "{line}")?;
                    }
                }
            }

            for url in &elem.package_homepage {
                if url.is_empty() {
                    continue;
                }
                write!(writer, "  Homepage: ")?;
                if hyperlinks {
                    write!(writer, "\x1b]8;;{url}\x07")?;
                    write!(writer, "{}", url.underline())?;
                    writeln!(writer, "\x1b]8;;\x07")?;
                } else {
                    writeln!(writer, "{url}")?;
                }
            }

            if self.platforms && !elem.package_platforms.is_empty() {
                write!(writer, "  Platforms: ")?;
                // Write platforms without allocating a new joined string
                for (i, platform) in elem.package_platforms.iter().enumerate() {
                    if i > 0 {
                        write!(writer, ", ")?;
                    }
                    write!(writer, "{platform}")?;
                }
                writeln!(writer)?;
            }

            if let Some(position) = &elem.package_position {
                if !position.is_empty() && !nixpkgs_path.is_empty() {
                    if let Some(file_part) = position.split(':').next() {
                        if !file_part.is_empty() {
                            write!(writer, "  Defined at: ")?;
                            if hyperlinks {
                                let full_file_path = format!("file://{nixpkgs_path}/{file_part}");
                                write!(writer, "\x1b]8;;{full_file_path}\x07")?;
                                write!(writer, "{}", position.underline())?;
                                writeln!(writer, "\x1b]8;;\x07")?;
                            } else {
                                writeln!(writer, "{position}")?;
                            }
                        }
                    }
                }
            }
        }
        writer.flush()?;
        Ok(())
    }
}

/// Asynchronously caches search results to disk in the background
/// to avoid blocking the main thread.
fn cache_results_in_background(
    query: Vec<String>,
    channel: String,
    limit: u64,
    _cache_duration: Option<u64>,
    results: Vec<SearchResult>,
) {
    // Create a new function to avoid borrowing self
    fn get_cache_path(query: &[String], channel: &str, limit: u64) -> Result<PathBuf> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let query_hash = {
            let mut hasher = DefaultHasher::new();
            query.hash(&mut hasher);
            channel.hash(&mut hasher);
            limit.hash(&mut hasher);
            hasher.finish()
        };

        let cache_dir = get_home_dir().join(CACHE_DIR);

        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir)?;
        }

        Ok(cache_dir.join(format!("search_{channel}_{query_hash}.{CACHE_FILE_EXT}")))
    }

    match get_cache_path(&query, &channel, limit) {
        Ok(cache_path) => {
            let cached = CachedResults {
                timestamp: SystemTime::now(),
                channel,
                query,
                results,
            };

            // Create parent directories if needed
            if let Some(parent) = cache_path.parent() {
                if !parent.exists() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        warn!("Failed to create cache directory: {}", e);
                        return;
                    }
                }
            }

            // Use atomic file operation with temp file
            let temp_path = cache_path.with_extension("tmp");

            match File::create(&temp_path) {
                Ok(file) => {
                    let writer = BufWriter::with_capacity(32768, file);
                    if let Err(e) = serde_json::to_writer(writer, &cached) {
                        warn!("Failed to serialize cache results: {}", e);
                        return;
                    }

                    if let Err(e) = fs::rename(temp_path, cache_path) {
                        warn!("Failed to move temp cache file: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to create cache file: {}", e);
                }
            }
        }
        Err(e) => {
            warn!("Failed to determine cache path: {}", e);
        }
    }
}

/// Verifies if a NixOS branch/channel is supported
/// by checking against known patterns and deprecated versions.
fn supported_branch<S: AsRef<str>>(branch: S) -> bool {
    let branch = branch.as_ref();

    // Fast path for common case
    if branch == "nixos-unstable" {
        return true;
    }

    // Check against deprecated versions list
    if DEPRECATED_VERSIONS.contains(&branch) {
        warn!("Channel {} is deprecated and not supported", branch);
        return false;
    }

    NIXOS_VERSION_REGEX.is_match(branch)
}

#[test]
fn test_supported_branch() {
    assert!(supported_branch("nixos-unstable"));
    assert!(!supported_branch("nixos-unstable-small"));
    assert!(!supported_branch("nixos-24.05")); // Now deprecated
    assert!(supported_branch("nixos-24.11"));
    assert!(!supported_branch("24.05"));
    assert!(!supported_branch("nixpkgs-darwin"));
    assert!(!supported_branch("nixpks-21.11-darwin"));
}
