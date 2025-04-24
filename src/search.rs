use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Instant, SystemTime};

use color_eyre::eyre::{bail, Context, Result};
use elasticsearch_dsl::*;
use interface::SearchArgs;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace, warn};

use crate::*;

const DEPRECATED_VERSIONS: &[&str] = &["nixos-24.05"];
const DEFAULT_CACHE_DURATION: u64 = 3600; // 1 hour in seconds
const CACHE_DIR: &str = ".cache/nh";

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
    package_hydra: (),
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

macro_rules! print_hyperlink {
    ($text:expr, $link:expr) => {
        print!("\x1b]8;;{}\x07", $link);
        print!("{}", $text.underline());
        println!("\x1b]8;;\x07");
    };
}

fn get_home_dir() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

impl SearchArgs {
    pub fn run(&self) -> Result<()> {
        trace!("args: {self:?}");

        if !supported_branch(&self.channel) {
            bail!("Channel {} is not supported!", self.channel);
        }

        // Check for cached results if caching is enabled
        if self.use_cache {
            if let Some(results) = self.get_cached_results()? {
                self.display_results(results)?;
                return Ok(());
            }
        }

        // Try to get nixpkgs path in parallel while we do the search
        let nixpkgs_path = std::thread::spawn(|| {
            std::process::Command::new("nix")
                .stderr(Stdio::inherit())
                .args(["eval", "-f", "<nixpkgs>", "path"])
                .output()
        });

        let query_s = self.query.join(" ");
        debug!(?query_s);

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
                            query_s.clone(),
                        )
                        .r#type(TextQueryType::CrossFields)
                        .analyzer("whitespace")
                        .auto_generate_synonyms_phrase_query(false)
                        .operator(Operator::And),
                    )
                    .query(
                        Query::wildcard("package_attr_name", format!("*{}*", &query_s))
                            .case_insensitive(true),
                    ),
            ),
        );

        println!(
            "Querying search.nixos.org, with channel {}...",
            self.channel
        );
        let then = Instant::now();

        let documents = if let Ok(docs) = self.perform_elasticsearch_search(&query) {
            docs
        } else if self.use_fallback
            && (self.fallback_file.is_some() || !self.fallback_endpoints.is_empty())
        {
            match self.try_fallback_methods(&query_s) {
                Ok(Some(docs)) => docs,
                _ => {
                    bail!("Failed to search using primary and fallback methods");
                }
            }
        } else {
            bail!("Failed to search and no fallback methods available");
        };

        let elapsed = then.elapsed();
        debug!(?elapsed);
        println!("Took {}ms", elapsed.as_millis());
        println!("Most relevant results at the end");
        println!();

        // Cache the results if caching is enabled
        if self.use_cache {
            if let Err(e) = self.cache_results(&documents) {
                warn!("Failed to cache results: {}", e);
            }
        }

        // Get nixpkgs path for displaying results
        let nixpkgs_path = match nixpkgs_path.join().unwrap() {
            Ok(output) => String::from_utf8(output.stdout)
                .unwrap_or_default()
                .trim()
                .to_string(),
            Err(_) => String::new(),
        };

        self.display_results_with_nixpkgs(documents, &nixpkgs_path)?;

        Ok(())
    }

    fn perform_elasticsearch_search(&self, query: &Search) -> Result<Vec<SearchResult>> {
        let client = reqwest::blocking::Client::new();
        let req = client
            // I guess 42 is the version of the backend API
            // TODO: have a GH action or something check if they updated this thing
            .post(format!(
                "https://search.nixos.org/backend/latest-42-{}/_search",
                self.channel
            ))
            .json(query)
            .header("User-Agent", format!("nh/{}", crate::NH_VERSION))
            // Hardcoded upstream
            // https://github.com/NixOS/nixos-search/blob/744ec58e082a3fcdd741b2c9b0654a0f7fda4603/frontend/src/index.js
            .basic_auth("aWVSALXpZv", Some("X8gPHnzL52wFEekuxsfQ9cSh"))
            .build()
            .context("building search query")?;

        debug!(?req);

        let response = client
            .execute(req)
            .context("querying the elasticsearch API")?;

        trace!(?response);

        let parsed_response: SearchResponse = response
            .json()
            .context("parsing response into the elasticsearch format")?;
        trace!(?parsed_response);

        let documents = parsed_response
            .documents::<SearchResult>()
            .context("parsing search document")?;

        Ok(documents)
    }

    fn try_fallback_methods(&self, query_s: &str) -> Result<Option<Vec<SearchResult>>> {
        // Try local file fallback first
        if let Some(fallback_file) = &self.fallback_file {
            if let Ok(Some(docs)) = self.try_file_fallback(fallback_file, query_s) {
                info!("Successfully retrieved results from fallback file");
                return Ok(Some(docs));
            }
        }

        // Try configured endpoints
        for endpoint in &self.fallback_endpoints {
            if let Ok(Some(docs)) = self.try_endpoint_fallback(endpoint, query_s) {
                info!("Successfully retrieved results from fallback endpoint");
                return Ok(Some(docs));
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

        let file = File::open(path).context("opening fallback file")?;
        let mut packages: Vec<SearchResult> =
            serde_json::from_reader(file).context("parsing fallback file")?;

        // Filter by query string
        let query_lower = query_s.to_lowercase();
        packages.retain(|pkg| {
            pkg.package_attr_name.to_lowercase().contains(&query_lower)
                || pkg.package_pname.to_lowercase().contains(&query_lower)
                || pkg
                    .package_description
                    .as_ref()
                    .map(|d| d.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
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
        endpoint: &str,
        query_s: &str,
    ) -> Result<Option<Vec<SearchResult>>> {
        let request_url = endpoint
            .replace("{channel}", &self.channel)
            .replace("{query}", query_s);

        match reqwest::blocking::get(&request_url) {
            Ok(response) => {
                if response.status().is_success() {
                    if let Ok(packages) = response.json::<Vec<SearchResult>>() {
                        // Filter packages by query if needed
                        let mut filtered_packages = packages;

                        // Limit results
                        if filtered_packages.len() > self.limit as usize {
                            filtered_packages.truncate(self.limit as usize);
                        }

                        if !filtered_packages.is_empty() {
                            return Ok(Some(filtered_packages));
                        }
                    }
                }
            }
            Err(e) => {
                debug!("Fallback endpoint {} failed: {}", endpoint, e);
            }
        }

        Ok(None)
    }

    fn get_cache_path(&self) -> Result<PathBuf> {
        let cache_dir = get_home_dir().join(CACHE_DIR);
        fs::create_dir_all(&cache_dir).context("creating cache directory")?;

        // Create a filename based on channel and query
        let query_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            self.query.hash(&mut hasher);
            self.channel.hash(&mut hasher);
            hasher.finish()
        };

        Ok(cache_dir.join(format!("search_{}_{}.json", self.channel, query_hash)))
    }

    fn get_cached_results(&self) -> Result<Option<Vec<SearchResult>>> {
        let cache_path = match self.get_cache_path() {
            Ok(path) => path,
            Err(_) => return Ok(None),
        };

        if !cache_path.exists() {
            return Ok(None);
        }

        let mut file = match File::open(&cache_path) {
            Ok(file) => file,
            Err(_) => return Ok(None),
        };

        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_err() {
            return Ok(None);
        }

        let cached: CachedResults = match serde_json::from_str(&contents) {
            Ok(cached) => cached,
            Err(_) => return Ok(None),
        };

        // Check if cache is still valid
        let cache_duration = self.cache_duration.unwrap_or(DEFAULT_CACHE_DURATION);
        match cached.timestamp.elapsed() {
            Ok(elapsed) if elapsed.as_secs() <= cache_duration => {
                info!(
                    "Using cached results from {} seconds ago",
                    elapsed.as_secs()
                );
                Ok(Some(cached.results))
            }
            _ => {
                // Cache is expired or timestamp error
                Ok(None)
            }
        }
    }

    fn cache_results(&self, results: &[SearchResult]) -> Result<()> {
        let cache_path = self.get_cache_path()?;

        let cached = CachedResults {
            timestamp: SystemTime::now(),
            channel: self.channel.clone(),
            query: self.query.clone(),
            results: results.to_vec(),
        };

        let json = serde_json::to_string(&cached)?;
        let mut file = File::create(cache_path)?;
        file.write_all(json.as_bytes())?;

        Ok(())
    }

    fn display_results(&self, results: Vec<SearchResult>) -> Result<()> {
        // Try to get nixpkgs path for displaying results
        let nixpkgs_path = match std::process::Command::new("nix")
            .stderr(Stdio::inherit())
            .args(["eval", "-f", "<nixpkgs>", "path"])
            .output()
        {
            Ok(output) => String::from_utf8(output.stdout)
                .unwrap_or_default()
                .trim()
                .to_string(),
            Err(_) => String::new(),
        };

        self.display_results_with_nixpkgs(results, &nixpkgs_path)
    }

    fn display_results_with_nixpkgs(
        &self,
        results: Vec<SearchResult>,
        nixpkgs_path: &str,
    ) -> Result<()> {
        let hyperlinks = supports_hyperlinks::supports_hyperlinks();
        debug!(?hyperlinks);

        for elem in results.iter().rev() {
            println!();
            use owo_colors::OwoColorize;
            trace!("{elem:#?}");

            print!("{}", elem.package_attr_name.blue());
            let v = &elem.package_pversion;
            if !v.is_empty() {
                print!(" ({})", v.green());
            }

            println!();

            if let Some(ref desc) = elem.package_description {
                let desc = desc.replace('\n', " ");
                for line in textwrap::wrap(&desc, textwrap::Options::with_termwidth()) {
                    println!("  {}", line);
                }
            }

            for url in elem.package_homepage.iter() {
                print!("  Homepage: ");
                if hyperlinks {
                    print_hyperlink!(url, url);
                } else {
                    println!("{}", url);
                }
            }

            if self.platforms && !elem.package_platforms.is_empty() {
                println!("  Platforms: {}", elem.package_platforms.join(", "));
            }

            if let Some(position) = &elem.package_position {
                if !nixpkgs_path.is_empty() {
                    let position = position.split(':').next().unwrap();
                    print!("  Defined at: ");
                    if hyperlinks {
                        let position_trimmed = position
                            .split(':')
                            .next()
                            .expect("Removing line number from position");

                        print_hyperlink!(
                            position,
                            format!("file://{}/{}", nixpkgs_path, position_trimmed)
                        );
                    } else {
                        println!("{}", position);
                    }
                }
            }
        }

        Ok(())
    }
}

fn supported_branch<S: AsRef<str>>(branch: S) -> bool {
    let branch = branch.as_ref();

    // Check against deprecated versions list
    if DEPRECATED_VERSIONS.contains(&branch) {
        warn!("Channel {} is deprecated and not supported", branch);
        return false;
    }

    if branch == "nixos-unstable" {
        return true;
    }

    let re = Regex::new(r"nixos-[0-9]+\.[0-9]+").unwrap();
    re.is_match(branch)
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
