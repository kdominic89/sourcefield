//! Collect configured public metadata with explicit provenance and redacted failures.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod diagnostics;
pub use diagnostics::CollectorWarning;

use std::collections::BTreeSet;

use chrono::{Duration, Utc};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT},
};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

use sourcefield_core::{
    AccountSnapshot, ActivityDay, Config, ContributionSnapshot, DataStatus, PackageSnapshot,
    RepositorySnapshot, Snapshot, SnapshotMode, SourceStatus,
};

const GITHUB_API: &str = "https://api.github.com";
const GITHUB_GRAPHQL: &str = "https://api.github.com/graphql";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

const NUGET_INDEX: &str = "https://api.nuget.org/v3/index.json";

#[derive(Debug, Error)]
/// Bounded errors; upstream response bodies are never retained.
pub enum CollectorError {
    /// Transport or response decoding failed.
    #[error("HTTP transport or decoding failed")]
    Client,
    /// An upstream returned a non-success status.
    #[error("upstream HTTP status {status}")]
    Http {
        /// Status without URL or response payload.
        status: StatusCode,
    },
    /// GraphQL rejected the request.
    #[error("GitHub GraphQL returned errors")]
    GraphQl,
    /// An expected response field was absent or invalid.
    #[error("unexpected API response: {0}")]
    Response(String),
}

impl From<reqwest::Error> for CollectorError {
    fn from(_: reqwest::Error) -> Self {
        // Transport errors can contain URLs; public diagnostics keep only the category.
        Self::Client
    }
}

#[derive(Clone)]
/// Public metadata collector with isolated optional aggregate credentials.
pub struct Collector {
    client: Client,
    token: Option<String>,
    private_token: Option<String>,
    github_api: String,
    graphql_api: String,
    nuget_api: String,
}

impl Collector {
    /// Create a collector using a public-metadata token, if supplied.
    pub fn new(token: Option<String>) -> Result<Self, CollectorError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("sourcefield-profile/0.3"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );

        let client = Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(24))
            .connect_timeout(std::time::Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        Ok(Self {
            client,
            token: token.filter(|value| !value.trim().is_empty()),
            private_token: None,
            github_api: GITHUB_API.into(),
            graphql_api: GITHUB_GRAPHQL.into(),
            nuget_api: NUGET_INDEX.into(),
        })
    }

    /// Set credentials used exclusively for the explicitly requested owned-private count.
    pub fn with_private_token(mut self, token: Option<String>) -> Self {
        self.private_token = token.filter(|value| !value.trim().is_empty());
        self
    }

    /// Collect configured sources, marking every incomplete source and redacting failures.
    ///
    /// `private_counts` is the caller's already-authorized effective intent. Organizations
    /// never request a private user aggregate, regardless of this flag.
    pub async fn collect(
        &self,
        config: &Config,
        private_counts: bool,
    ) -> Result<Snapshot, CollectorError> {
        let mut snapshot = Snapshot {
            mode: SnapshotMode::Live,
            fetched_at: Utc::now().to_rfc3339(),
            ..Snapshot::default()
        };

        let collect_user = config.profile.variant == sourcefield_core::ProfileVariant::Personal
            && !config.collection.github_user.is_empty();

        // An organization has no user contribution calendar or owned-private user aggregate.
        if collect_user {
            snapshot.user = self
                .fetch_account(&config.collection.github_user, AccountKind::User)
                .await?;
            record(&mut snapshot, "github:user", true);
        }

        for organization in &config.collection.github_organizations {
            match self
                .fetch_account(organization, AccountKind::Organization)
                .await
            {
                Ok(account) => {
                    snapshot.organizations.push(account);
                    record(&mut snapshot, &format!("github:org:{organization}"), true);
                }

                Err(_) => record(&mut snapshot, &format!("github:org:{organization}"), false),
            }
        }

        if config.collection.discover_public_repositories {
            let mut repositories = if collect_user {
                let inventory = self
                    .list_repositories(
                        &config.collection.github_user,
                        AccountKind::User,
                        config.collection.repository_limit,
                    )
                    .await?;
                record(
                    &mut snapshot,
                    &format!("github:repositories:{}", config.collection.github_user),
                    inventory.complete,
                );

                if !inventory.complete {
                    snapshot
                        .warnings
                        .push(CollectorWarning::RepositoryLimit.to_string());
                }

                inventory.repositories
            } else {
                Vec::new()
            };

            for organization in &config.collection.github_organizations {
                match self
                    .list_repositories(
                        organization,
                        AccountKind::Organization,
                        config.collection.repository_limit,
                    )
                    .await
                {
                    Ok(mut inventory) => {
                        record(
                            &mut snapshot,
                            &format!("github:repositories:{organization}"),
                            inventory.complete,
                        );

                        if !inventory.complete {
                            snapshot
                                .warnings
                                .push(CollectorWarning::RepositoryLimit.to_string());
                        }

                        repositories.append(&mut inventory.repositories);
                    }
                    Err(_) => record(
                        &mut snapshot,
                        &format!("github:repositories:{organization}"),
                        false,
                    ),
                }
            }

            repositories.retain(|repository| {
                (config.collection.include_forks || !repository.fork)
                    && (config.collection.include_archived || !repository.archived)
            });
            repositories.sort_by(|left, right| left.full_name.cmp(&right.full_name));
            repositories.dedup_by(|left, right| left.full_name == right.full_name);
            snapshot.repositories = repositories;
        }

        if collect_user && config.collection.collect_contributions {
            match self
                .fetch_contributions(&config.collection.github_user)
                .await
            {
                Ok(contributions) => {
                    snapshot.contributions = Some(contributions);
                    record(&mut snapshot, "github:contributions", true);
                }

                _ => record(&mut snapshot, "github:contributions", false),
            }
        }

        if collect_user && private_counts {
            match self
                .fetch_private_count(&config.collection.github_user)
                .await
            {
                Ok(count) => {
                    snapshot.private_repository_count = Some(count);
                    record(&mut snapshot, "github:private-count", true);
                }

                Err(_) => record(&mut snapshot, "github:private-count", false),
            }
        }

        self.collect_registry(config, &mut snapshot).await;

        for package in config
            .publications
            .iter()
            .flat_map(|publication| &publication.packages)
        {
            let complete = snapshot.packages.iter().any(|value| {
                value.id.eq_ignore_ascii_case(&package.id)
                    && value.version.is_some()
                    && value.total_downloads.is_some()
            });

            record(
                &mut snapshot,
                &format!("nuget:{}", package.id.to_ascii_lowercase()),
                complete,
            );
        }

        Ok(snapshot)
    }

    /// Count token-visible private repositories owned by the configured user.
    /// Organization repositories are excluded.
    async fn fetch_private_count(&self, login: &str) -> Result<u32, CollectorError> {
        let token = self
            .private_token
            .as_ref()
            .ok_or_else(|| CollectorError::Response("private-count credentials missing".into()))?;

        let query = concat!(
            "query($login:String!){user(login:$login){",
            "repositories(first:1,privacy:PRIVATE,ownerAffiliations:[OWNER]){totalCount}}}"
        );

        let body = json!({"query": query, "variables": {"login": login}});
        let request = self
            .client
            .post(&self.graphql_api)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .json(&body);
        let response = send_read_request(request).await?;

        let value: Value = decode(response, &self.graphql_api).await?;
        if value.get("errors").is_some() {
            return Err(CollectorError::GraphQl);
        }

        value
            .pointer("/data/user/repositories/totalCount")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| CollectorError::Response("invalid private count".into()))
    }

    async fn fetch_account(
        &self,
        login: &str,
        kind: AccountKind,
    ) -> Result<AccountSnapshot, CollectorError> {
        let endpoint = match kind {
            AccountKind::User => format!("/users/{login}"),
            AccountKind::Organization => format!("/orgs/{login}"),
        };

        let value: GithubAccount = self.get_github(&endpoint).await?;
        Ok(AccountSnapshot {
            login: value.login,
            display_name: value.name,
            profile_url: Some(value.html_url),
            public_repositories: value.public_repos,
            followers: match (kind, value.followers) {
                (AccountKind::User, None) => {
                    return Err(CollectorError::Response(
                        "missing user follower count".into(),
                    ));
                }
                (_, followers) => followers.unwrap_or(0),
            },
        })
    }

    async fn list_repositories(
        &self,
        login: &str,
        kind: AccountKind,
        limit: usize,
    ) -> Result<RepositoryInventory, CollectorError> {
        let base = match kind {
            AccountKind::User => format!("/users/{login}/repos?type=owner&sort=pushed"),
            AccountKind::Organization => format!("/orgs/{login}/repos?type=public&sort=pushed"),
        };

        let mut repositories = Vec::new();
        let maximum = limit.clamp(1, 1_000);
        let mut complete = false;

        for page in 1..=10 {
            let endpoint = format!("{base}&per_page=100&page={page}");
            let values: Vec<GithubRepository> = self.get_github(&endpoint).await?;
            let page_size = values.len();

            if page_size > 100
                || values
                    .iter()
                    .any(|value| !value.owner.login.eq_ignore_ascii_case(login) || value.private)
            {
                return Err(CollectorError::Response(
                    "invalid public repository page".into(),
                ));
            }

            repositories.extend(values.into_iter().map(RepositorySnapshot::from));
            // A full final page cannot prove exhaustion; reaching either bound is incomplete.
            complete = page_size < 100 && repositories.len() <= maximum;

            if page_size < 100 || repositories.len() >= maximum {
                break;
            }
        }

        repositories.truncate(maximum);

        Ok(RepositoryInventory {
            repositories,
            complete,
        })
    }

    /// Discover the registry once and merge inventories by normalized ID without quadratic scans.
    async fn collect_registry(&self, config: &Config, snapshot: &mut Snapshot) {
        if config.publications.is_empty() {
            return;
        }

        let owners = config
            .publications
            .iter()
            .filter_map(|group| sourcefield_core::publication_owner(config, group))
            .map(str::to_ascii_lowercase)
            .collect::<BTreeSet<_>>();

        let endpoint = match self.nuget_search_endpoint().await {
            Ok(endpoint) => endpoint,
            Err(_) => {
                record(snapshot, "nuget:service-index", false);

                for owner in owners {
                    record(snapshot, &format!("nuget:owner:{owner}"), false);
                }

                return;
            }
        };

        let packages = self
            .collect_packages(config, &mut snapshot.warnings, &endpoint)
            .await;

        let mut packages = packages
            .into_iter()
            .map(|package| (package.id.to_ascii_lowercase(), package))
            .collect::<std::collections::BTreeMap<_, _>>();

        for owner in owners {
            match self
                .discover_packages(config, &owner, Some(&endpoint))
                .await
            {
                Ok(inventory) => {
                    packages.extend(
                        inventory
                            .into_iter()
                            .map(|package| (package.id.to_ascii_lowercase(), package)),
                    );
                    record(snapshot, &format!("nuget:owner:{owner}"), true);
                }

                Err(_) => record(snapshot, &format!("nuget:owner:{owner}"), false),
            }
        }

        snapshot.packages = packages.into_values().collect();
    }

    async fn collect_packages(
        &self,
        config: &Config,
        warnings: &mut Vec<String>,
        endpoint: &str,
    ) -> Vec<PackageSnapshot> {
        let mut seen = BTreeSet::new();
        let mut packages = Vec::new();
        if config
            .publications
            .iter()
            .all(|publication| publication.packages.is_empty())
        {
            return packages;
        }

        for package in config
            .publications
            .iter()
            .flat_map(|publication| publication.packages.iter())
        {
            if !seen.insert(package.id.to_ascii_lowercase()) {
                continue;
            }

            let expected_owner = config
                .publications
                .iter()
                .find(|group| {
                    group
                        .packages
                        .iter()
                        .any(|item| item.id.eq_ignore_ascii_case(&package.id))
                })
                .and_then(|group| sourcefield_core::publication_owner(config, group));

            let request = self.client.get(endpoint).query(&[
                ("q", format!("packageid:{}", package.id)),
                ("prerelease", "true".to_string()),
                ("semVerLevel", "2.0.0".to_string()),
                ("take", "1".to_string()),
            ]);

            match send_read_request(request).await {
                Ok(response) => {
                    let url = response.url().to_string();
                    match decode::<NugetSearchResponse>(response, &url).await {
                        Ok(result) => {
                            if let Some(found) = result.data.into_iter().find(|value| {
                                value.id.eq_ignore_ascii_case(&package.id)
                                    && !value.version.is_empty()
                                    && expected_owner
                                        .is_none_or(|owner| value.owners.contains(owner))
                            }) {
                                packages.push(PackageSnapshot {
                                    id: package.id.clone(),
                                    owner: expected_owner.map(str::to_owned),
                                    version: Some(found.version),
                                    total_downloads: found.total_downloads,
                                    updated_at: None,
                                    url: Some(package.url.clone()),
                                });
                            } else {
                                warnings.push(
                                    CollectorWarning::NugetNoExactMatch(&package.id).to_string(),
                                );
                                packages.push(PackageSnapshot {
                                    id: package.id.clone(),
                                    url: Some(package.url.clone()),
                                    ..PackageSnapshot::default()
                                });
                            }
                        }

                        Err(_) => {
                            warnings.push(
                                CollectorWarning::NugetMetadataUnavailable(&package.id).to_string(),
                            );
                        }
                    }
                }

                Err(_) => warnings
                    .push(CollectorWarning::NugetMetadataUnavailable(&package.id).to_string()),
            }
        }

        packages.sort_by(|left, right| left.id.cmp(&right.id));
        packages
    }

    /// Enumerate the exact NuGet owner and admit only configured package families.
    /// Pagination fails closed so a truncated inventory never masquerades as complete.
    async fn discover_packages(
        &self,
        config: &Config,
        owner: &str,
        endpoint: Option<&str>,
    ) -> Result<Vec<PackageSnapshot>, CollectorError> {
        // Reuse one discovery result for the whole collection, while focused probes can resolve their own.
        let discovered;
        let endpoint = if let Some(endpoint) = endpoint {
            endpoint
        } else {
            discovered = self.nuget_search_endpoint().await?;
            &discovered
        };

        let mut packages = Vec::new();
        let mut seen = BTreeSet::new();
        let mut skip = 0;
        let mut expected_total = None;
        loop {
            let request = self.client.get(endpoint).query(&[
                ("q", format!("owner:{owner}")),
                ("prerelease", "true".into()),
                ("semVerLevel", "2.0.0".into()),
                ("skip", skip.to_string()),
                ("take", "100".into()),
            ]);
            let response = send_read_request(request).await?;

            let result: NugetSearchResponse = decode(response, endpoint).await?;
            let total = result
                .total_hits
                .ok_or_else(|| CollectorError::Response("NuGet totalHits missing".into()))?;

            if expected_total.is_some_and(|expected| expected != total)
                || total > 1000
                || skip + result.data.len() > total
                || result.data.len() > 100
                || (result.data.is_empty() && skip < total)
            {
                return Err(CollectorError::Response(
                    "NuGet inventory incomplete".into(),
                ));
            }

            expected_total = Some(total);
            let count = result.data.len();
            for package in result.data {
                if !seen.insert(package.id.to_ascii_lowercase()) {
                    return Err(CollectorError::Response(
                        "NuGet duplicate page entry".into(),
                    ));
                }

                if package.owners.contains(owner)
                    && !sourcefield_core::valid_package_id(&package.id)
                {
                    return Err(CollectorError::Response("invalid NuGet package ID".into()));
                }

                if !package.owners.contains(owner)
                    || !config.publications.iter().any(|group| {
                        sourcefield_core::publication_owner(config, group)
                            .is_some_and(|expected| expected.eq_ignore_ascii_case(owner))
                            && sourcefield_core::package_matches(group, &package.id)
                    })
                {
                    continue;
                }

                if !sourcefield_core::valid_package_id(&package.id)
                    || package.version.is_empty()
                    || package.total_downloads.is_none()
                {
                    return Err(CollectorError::Response(
                        "invalid NuGet package metadata".into(),
                    ));
                }

                packages.push(PackageSnapshot {
                    url: Some(format!("https://www.nuget.org/packages/{}/", package.id)),
                    id: package.id,
                    owner: Some(owner.to_string()),
                    version: Some(package.version),
                    total_downloads: package.total_downloads,
                    updated_at: None,
                });
            }

            skip += count;
            if skip >= total {
                break;
            }
        }

        Ok(packages)
    }

    /// Resolve the versioned NuGet search resource from the authoritative service index.
    async fn nuget_search_endpoint(&self) -> Result<String, CollectorError> {
        let response = send_read_request(self.client.get(&self.nuget_api)).await?;
        let index: Value = decode(response, &self.nuget_api).await?;
        let endpoint = index
            .get("resources")
            .and_then(Value::as_array)
            .and_then(|resources| {
                resources.iter().find(|resource| {
                    resource
                        .get("@type")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| {
                            kind == "SearchQueryService" || kind.starts_with("SearchQueryService/")
                        })
                })
            })
            .and_then(|resource| resource.get("@id"))
            .and_then(Value::as_str)
            .ok_or_else(|| CollectorError::Response("NuGet search resource missing".into()))?;

        let parsed = reqwest::Url::parse(endpoint)
            .map_err(|_| CollectorError::Response("invalid NuGet resource URL".into()))?;

        let index_url = reqwest::Url::parse(&self.nuget_api)
            .map_err(|_| CollectorError::Response("invalid NuGet index URL".into()))?;

        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(CollectorError::Response(
                "invalid NuGet resource URL".into(),
            ));
        }

        if parsed.scheme() != "https"
            && !(index_url.scheme() == "http"
                && index_url.host_str() == Some("127.0.0.1")
                && parsed.host_str() == Some("127.0.0.1"))
        {
            return Err(CollectorError::Response(
                "NuGet resource requires HTTPS".into(),
            ));
        }

        Ok(endpoint.to_string())
    }

    async fn fetch_contributions(
        &self,
        login: &str,
    ) -> Result<ContributionSnapshot, CollectorError> {
        if self.token.is_none() {
            return Err(CollectorError::Response(
                "contribution credentials missing".into(),
            ));
        }

        let to = Utc::now();
        let from = to - Duration::days(364);
        let query = r#"
query SourcefieldProfile($login: String!, $from: DateTime!, $to: DateTime!) {
  user(login: $login) {
    contributionsCollection(from: $from, to: $to) {
      totalCommitContributions
      totalIssueContributions
      totalPullRequestContributions
      totalPullRequestReviewContributions
      restrictedContributionsCount
      contributionCalendar {
        totalContributions
        weeks {
          contributionDays {
            date
            contributionCount
            contributionLevel
          }
        }
      }
    }
  }
}

"#;

        let body = json!({
            "query": query,
            "variables": {
                "login": login,
                "from": from.to_rfc3339(),
                "to": to.to_rfc3339(),
            }
        });

        let response =
            send_read_request(self.authorized(self.client.post(&self.graphql_api).json(&body)))
                .await?;

        let value: Value = decode(response, &self.graphql_api).await?;
        if value.get("errors").is_some() {
            return Err(CollectorError::GraphQl);
        }

        let user = value
            .pointer("/data/user")
            .ok_or_else(|| CollectorError::Response("missing GraphQL user".to_string()))?;

        let collection = user.get("contributionsCollection").ok_or_else(|| {
            CollectorError::Response("missing contributionsCollection".to_string())
        })?;

        for path in [
            "/contributionCalendar/totalContributions",
            "/totalCommitContributions",
            "/totalIssueContributions",
            "/totalPullRequestContributions",
            "/totalPullRequestReviewContributions",
            "/restrictedContributionsCount",
        ] {
            if collection
                .pointer(path)
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .is_none()
            {
                return Err(CollectorError::Response(
                    "invalid contribution counter".into(),
                ));
            }
        }

        if collection
            .pointer("/contributionCalendar/weeks")
            .and_then(Value::as_array)
            .is_none()
        {
            return Err(CollectorError::Response(
                "missing contribution calendar".into(),
            ));
        }

        let mut days = Vec::new();
        for week in collection
            .pointer("/contributionCalendar/weeks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let values = week
                .get("contributionDays")
                .and_then(Value::as_array)
                .ok_or_else(|| CollectorError::Response("missing contribution days".into()))?;

            for day in values {
                let date = day
                    .get("date")
                    .and_then(Value::as_str)
                    .filter(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
                    .ok_or_else(|| CollectorError::Response("invalid contribution date".into()))?;

                let count = day
                    .get("contributionCount")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| CollectorError::Response("invalid contribution count".into()))?;

                let level = day
                    .get("contributionLevel")
                    .and_then(Value::as_str)
                    .filter(|value| {
                        matches!(
                            *value,
                            "NONE"
                                | "FIRST_QUARTILE"
                                | "SECOND_QUARTILE"
                                | "THIRD_QUARTILE"
                                | "FOURTH_QUARTILE"
                        )
                    })
                    .ok_or_else(|| CollectorError::Response("invalid contribution level".into()))?;

                days.push(ActivityDay {
                    date: date.to_string(),
                    count,
                    level: contribution_level(level),
                });
            }
        }

        days.sort_by(|left, right| left.date.cmp(&right.date));

        let contributions = ContributionSnapshot {
            total: u32_at(collection.pointer("/contributionCalendar/totalContributions")),
            commits: u32_at(collection.get("totalCommitContributions")),
            issues: u32_at(collection.get("totalIssueContributions")),
            pull_requests: u32_at(collection.get("totalPullRequestContributions")),
            reviews: u32_at(collection.get("totalPullRequestReviewContributions")),
            restricted: u32_at(collection.get("restrictedContributionsCount")),
            days,
        };

        Ok(contributions)
    }

    async fn get_github<T: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
    ) -> Result<T, CollectorError> {
        let url = format!("{}{endpoint}", self.github_api);
        let response = send_read_request(self.authorized(self.client.get(&url))).await?;
        decode(response, &url).await
    }

    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            request.header(AUTHORIZATION, format!("Bearer {token}"))
        } else {
            request
        }
    }
}

/// Bounded inventory with explicit evidence of pagination exhaustion.
#[derive(Debug)]
struct RepositoryInventory {
    repositories: Vec<RepositorySnapshot>,
    complete: bool,
}

/// Send an idempotent read request with bounded transient retries and redacted errors.
///
/// Callers must supply GET requests or read-only GraphQL queries using clients with
/// redirects disabled. Requests retain their original URL and headers across retries.
/// Three attempts share a 24-second deadline, including server-directed waits. A delay
/// that does not fit is never shortened; the last response is returned to the caller.
///
/// Server delay handling follows GitHub's [rate-limit guidance](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api).
pub async fn send_read_request(
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, CollectorError> {
    send_with_policy(request, RetryPolicy::default()).await
}

#[derive(Clone, Copy)]
struct RetryPolicy {
    budget: std::time::Duration,
    initial_delay: std::time::Duration,
    attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            budget: std::time::Duration::from_secs(24),
            initial_delay: std::time::Duration::from_millis(250),
            attempts: 3,
        }
    }
}

/// Retry only transport failures and explicitly classified transient HTTP responses.
async fn send_with_policy(
    request: reqwest::RequestBuilder,
    policy: RetryPolicy,
) -> Result<reqwest::Response, CollectorError> {
    let deadline = tokio::time::Instant::now() + policy.budget;

    for attempt in 0..policy.attempts {
        let attempt_request =
            request_with_remaining_budget(&request, deadline, tokio::time::Instant::now())?;

        let result = attempt_request.send().await;
        let fallback_delay = policy.initial_delay.saturating_mul(1_u32 << attempt);
        let delay = match &result {
            Ok(response) => retry_delay(response.status(), response.headers(), fallback_delay),
            Err(error) if error.is_connect() || error.is_timeout() => Some(fallback_delay),
            Err(_) => None,
        };

        if attempt + 1 == policy.attempts || delay.is_none() {
            return result.map_err(CollectorError::from);
        }

        let delay = delay.ok_or(CollectorError::Client)?;

        // Keep enough time for a real next attempt; never cap Retry-After or reset delays.
        if delay >= deadline.saturating_duration_since(tokio::time::Instant::now()) {
            return result.map_err(CollectorError::from);
        }

        drop(result);
        tokio::time::sleep(delay).await;
    }

    Err(CollectorError::Client)
}

/// Admit one attempt using the original deadline, including time spent on earlier responses.
fn request_with_remaining_budget(
    request: &reqwest::RequestBuilder,
    deadline: tokio::time::Instant,
    now: tokio::time::Instant,
) -> Result<reqwest::RequestBuilder, CollectorError> {
    let remaining = deadline.saturating_duration_since(now);

    if remaining.is_zero() {
        return Err(CollectorError::Client);
    }

    // Reqwest carries this timeout into body reads; headers cannot reset the shared budget.
    let attempt = request
        .try_clone()
        .ok_or(CollectorError::Client)?
        .timeout(remaining);

    Ok(attempt)
}

/// Interpret server retry instructions conservatively, without logging headers or bodies.
fn retry_delay(
    status: StatusCode,
    headers: &HeaderMap,
    fallback: std::time::Duration,
) -> Option<std::time::Duration> {
    retry_delay_at(status, headers, fallback, Utc::now().timestamp())
}

/// Evaluate every server timestamp against one instant, avoiding clock drift between headers.
fn retry_delay_at(
    status: StatusCode,
    headers: &HeaderMap,
    fallback: std::time::Duration,
    now: i64,
) -> Option<std::time::Duration> {
    let rate_limited = headers
        .get("x-ratelimit-remaining")
        .is_some_and(|value| value == "0");
    let has_retry_after = headers.contains_key("retry-after");
    let eligible = matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
        || (status == StatusCode::FORBIDDEN && (rate_limited || has_retry_after));

    if !eligible {
        return None;
    }

    let mut delay = None;

    if let Some(value) = headers.get("retry-after") {
        let value = value.to_str().ok()?;
        let seconds = if let Ok(seconds) = value.parse::<u64>() {
            seconds
        } else {
            chrono::DateTime::parse_from_rfc2822(value)
                .ok()?
                .timestamp()
                .saturating_sub(now)
                .max(0) as u64
        };

        delay = Some(std::time::Duration::from_secs(seconds));
    }

    if rate_limited {
        let reset = headers
            .get("x-ratelimit-reset")?
            .to_str()
            .ok()?
            .parse::<i64>()
            .ok()?;
        let seconds = reset.saturating_sub(now).max(0) as u64;
        let reset_delay = std::time::Duration::from_secs(seconds);
        delay = Some(delay.map_or(reset_delay, |delay| delay.max(reset_delay)));
    }

    // GitHub secondary limits require at least a minute when the server supplies no deadline.
    if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::FORBIDDEN {
        return Some(delay.unwrap_or(std::time::Duration::from_secs(60)));
    }

    Some(delay.unwrap_or(fallback))
}

#[derive(Debug, Clone, Copy)]
enum AccountKind {
    User,
    Organization,
}

#[derive(Debug, Deserialize)]
struct GithubAccount {
    login: String,
    name: Option<String>,
    html_url: String,
    public_repos: u32,
    #[serde(default)]
    followers: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GithubOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GithubRepository {
    #[serde(default)]
    private: bool,
    owner: GithubOwner,
    name: String,
    full_name: String,
    html_url: String,
    description: Option<String>,
    language: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    stargazers_count: u32,
    #[serde(default)]
    forks_count: u32,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    fork: bool,
    pushed_at: Option<String>,
}

impl From<GithubRepository> for RepositorySnapshot {
    fn from(value: GithubRepository) -> Self {
        Self {
            owner: value.owner.login,
            name: value.name,
            full_name: value.full_name,
            url: value.html_url,
            description: value.description,
            primary_language: value.language,
            topics: value.topics,
            stars: value.stargazers_count,
            forks: value.forks_count,
            archived: value.archived,
            fork: value.fork,
            pushed_at: value.pushed_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct NugetSearchResponse {
    #[serde(default, rename = "totalHits")]
    total_hits: Option<usize>,
    #[serde(default)]
    data: Vec<NugetPackage>,
}

#[derive(Debug, Deserialize)]
struct NugetPackage {
    #[serde(default)]
    owners: NugetOwners,
    id: String,
    version: String,
    #[serde(default, rename = "totalDownloads")]
    total_downloads: Option<u64>,
}

/// NuGet exposes owner names as either a string or an array.
#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum NugetOwners {
    Names(Vec<String>),
    Name(String),
    #[default]
    Missing,
}

impl NugetOwners {
    fn contains(&self, owner: &str) -> bool {
        match self {
            Self::Names(names) => names.iter().any(|name| name.eq_ignore_ascii_case(owner)),
            Self::Name(name) => name.eq_ignore_ascii_case(owner),
            Self::Missing => false,
        }
    }
}

async fn decode<T: for<'de> Deserialize<'de>>(
    mut response: reqwest::Response,
    _url: &str,
) -> Result<T, CollectorError> {
    let status = response.status();
    if !status.is_success() {
        return Err(CollectorError::Http { status });
    }

    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(CollectorError::Response(
            "response exceeds size limit".into(),
        ));
    }

    // Bound incremental reads even when the peer omits Content-Length or uses chunked transfer.
    let mut bytes = Vec::new();

    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
            return Err(CollectorError::Response(
                "response exceeds size limit".into(),
            ));
        }

        bytes.extend_from_slice(&chunk);
    }

    serde_json::from_slice(&bytes)
        .map_err(|_| CollectorError::Response("invalid response JSON".into()))
}

fn contribution_level(value: &str) -> u8 {
    match value {
        "FIRST_QUARTILE" => 1,
        "SECOND_QUARTILE" => 2,
        "THIRD_QUARTILE" => 3,
        "FOURTH_QUARTILE" => 4,
        _ => 0,
    }
}

fn u32_at(value: Option<&Value>) -> u32 {
    value.and_then(Value::as_u64).unwrap_or_default() as u32
}

/// Publish a stable source code instead of untrusted upstream diagnostics.
fn record(snapshot: &mut Snapshot, source: &str, complete: bool) {
    snapshot.sources.push(SourceStatus {
        source: source.into(),
        status: if complete {
            DataStatus::Live
        } else {
            DataStatus::Missing
        },
    });
    if !complete {
        snapshot.mode = SnapshotMode::Partial;
        snapshot
            .warnings
            .push(CollectorWarning::SourceUnavailable(source).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    /// Bound accept and subsequent socket I/O so a failed request cannot strand a join.
    fn accept_fixture(listener: &TcpListener) -> Option<std::net::TcpStream> {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    // Some platforms inherit O_NONBLOCK from the listener; use bounded blocking I/O.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
                        .unwrap();

                    return Some(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return None;
                    }

                    thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => panic!("fixture accept failed: {error}"),
            }
        }
    }

    /// Minimal real HTTP fixture; captured requests prove credential and query boundaries.
    fn fixture(
        responses: Vec<(u16, String)>,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        fixture_with_headers(responses, "")
    }

    fn fixture_with_headers(
        responses: Vec<(u16, String)>,
        headers: &'static str,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let fixture_endpoint = endpoint.clone();
        let worker = thread::spawn(move || {
            for (status, body) in responses {
                let body = body.replace("FIXTURE_ENDPOINT", &fixture_endpoint);

                let Some(mut stream) = accept_fixture(&listener) else {
                    return;
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();

                loop {
                    let mut chunk = [0; 4096];
                    let count = stream.read(&mut chunk).unwrap();
                    if count == 0 {
                        break;
                    }

                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);

                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }

                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                let _ = write!(
                    stream,
                    concat!(
                        "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\n",
                        "{}Content-Length: {}\r\nConnection: close\r\n\r\n{}"
                    ),
                    status,
                    headers,
                    body.len(),
                    body
                );
            }
        });

        (endpoint, requests, worker)
    }

    /// Synthetic authored input keeps tests independent from either real consumer profile.
    fn full_config() -> Config {
        serde_json::from_value(json!({
            "schema_version": 1,
            "profile": {
                "variant": "personal", "username": "owner", "display_name": "Owner",
                "organization": "example-labs", "headline": "Tools", "tagline": "Tools",
                "pages_url": "https://example.invalid", "source_url": "https://github.com/owner/profile"
            },
            "collection": {"github_user":"owner", "nuget_owner":"example-labs"},
            "render": {"width":1200,"height":1200},
            "publications":[{
                "id":"safe-migrations", "label":"Example.Migrations", "surface_label":"Migrations",
                "domain":"example-labs", "registry":"nuget", "anchor":[100,100], "summary":"Tools",
                "discovery_prefixes":["Example.Migrations"],
                "packages":[{"id":"Example.Caching", "family":"Caching", "url":"https://www.nuget.org/packages/Example.Caching"}]
            }]
        })).unwrap()
    }

    fn config() -> Config {
        let mut config = full_config();
        config.collection.github_organizations.clear();
        config.collection.discover_public_repositories = false;
        config.collection.collect_contributions = false;
        config.collection.collect_private_repository_count = true;
        config.publications.clear();
        config.collection.nuget_owner = None;

        config
    }

    fn account() -> String {
        json!({
            "login": "owner",
            "name": null,
            "html_url": "https://github.com/owner",
            "public_repos": 2,
            "followers": 0
        })
        .to_string()
    }

    #[tokio::test]
    async fn organization_collection_uses_only_public_org_endpoints_even_with_user_flags() {
        let (endpoint, requests, worker) = fixture(vec![(200, account()), (200, "[]".into())]);
        let mut collector = Collector::new(Some("public-token".into()))
            .unwrap()
            .with_private_token(Some("private-token".into()));

        collector.github_api = endpoint.clone();
        collector.graphql_api = endpoint;
        let mut config = config();
        config.profile.variant = sourcefield_core::ProfileVariant::Organization;
        config.collection.github_user.clear();
        config.collection.github_organizations = vec!["example-labs".into()];
        config.collection.discover_public_repositories = true;
        config.collection.collect_contributions = true;

        let snapshot = collector.collect(&config, true).await.unwrap();

        worker.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /orgs/example-labs HTTP/1.1"));
        assert!(requests[1].starts_with("GET /orgs/example-labs/repos?type=public&sort=pushed"));
        assert!(
            requests
                .iter()
                .all(|request| !request.contains("private-token"))
        );
        assert_eq!(snapshot.mode, SnapshotMode::Live);
        assert!(snapshot.user.login.is_empty());
        assert!(snapshot.private_repository_count.is_none());
        assert!(snapshot.contributions.is_none());
    }

    #[tokio::test]
    async fn failed_org_only_collection_is_partial_and_redacts_response() {
        let (endpoint, requests, worker) = fixture(vec![(403, "SECRET".into())]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;
        let mut config = config();
        config.profile.variant = sourcefield_core::ProfileVariant::Organization;
        config.collection.github_user.clear();
        config.collection.github_organizations = vec!["example-labs".into()];

        let snapshot = collector.collect(&config, false).await.unwrap();

        worker.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert_eq!(snapshot.mode, SnapshotMode::Partial);
        assert!(snapshot.organizations.is_empty());
        assert!(!serde_json::to_string(&snapshot).unwrap().contains("SECRET"));
    }

    #[tokio::test]
    async fn complete_public_collection_is_live_and_does_not_request_private_data() {
        let (endpoint, requests, worker) = fixture(vec![(200, account())]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;

        let snapshot = collector.collect(&config(), false).await.unwrap();

        worker.join().unwrap();
        assert_eq!(snapshot.mode, SnapshotMode::Live);
        assert_eq!(snapshot.private_repository_count, None);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn organization_failure_is_partial_and_response_secrets_are_not_persisted() {
        let (endpoint, _, worker) = fixture(vec![
            (200, account()),
            (403, "SECRET-TOKEN user@example.invalid".into()),
        ]);

        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;
        let mut config = config();
        config
            .collection
            .github_organizations
            .push("organization".into());

        let snapshot = collector.collect(&config, false).await.unwrap();

        worker.join().unwrap();
        assert_eq!(snapshot.mode, SnapshotMode::Partial);
        let public = serde_json::to_string(&snapshot).unwrap();
        assert!(!public.contains("SECRET-TOKEN"));
        assert!(!public.contains("user@example.invalid"));
        assert!(public.contains("github:org:organization"));
    }

    #[tokio::test]
    async fn private_count_is_independent_and_uses_only_isolated_credentials() {
        let (endpoint, requests, worker) = fixture(vec![
            (200, account()),
            (
                200,
                json!({"data":{"user":{"repositories":{"totalCount":7}}}}).to_string(),
            ),
        ]);

        let mut collector = Collector::new(Some("public-secret".into()))
            .unwrap()
            .with_private_token(Some("private-secret".into()));

        collector.github_api = endpoint.clone();
        collector.graphql_api = endpoint;

        let snapshot = collector.collect(&config(), true).await.unwrap();

        worker.join().unwrap();
        assert_eq!(snapshot.private_repository_count, Some(7));
        assert!(snapshot.contributions.is_none());
        let requests = requests.lock().unwrap();
        assert!(requests[0].contains("public-secret"));
        assert!(!requests[0].contains("private-secret"));
        assert!(requests[1].contains("private-secret"));
        assert!(!requests[1].contains("public-secret"));
        assert!(requests[1].contains("ownerAffiliations:[OWNER]"));
        assert!(!requests[1].contains("contributionsCollection"));
    }

    #[tokio::test]
    async fn malformed_private_count_stays_unknown_and_partial() {
        let (endpoint, _, worker) = fixture(vec![
            (200, account()),
            (
                200,
                json!({"data":{"user":{"repositories":{"totalCount":-1}}}}).to_string(),
            ),
        ]);

        let mut collector = Collector::new(None)
            .unwrap()
            .with_private_token(Some("secret".into()));

        collector.github_api = endpoint.clone();
        collector.graphql_api = endpoint;

        let snapshot = collector.collect(&config(), true).await.unwrap();

        worker.join().unwrap();
        assert_eq!(snapshot.private_repository_count, None);
        assert_eq!(snapshot.mode, SnapshotMode::Partial);
    }

    #[tokio::test]
    async fn missing_contribution_token_marks_requested_source_missing() {
        let (endpoint, requests, worker) = fixture(vec![(200, account())]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;
        let mut config = config();
        config.collection.collect_contributions = true;

        let snapshot = collector.collect(&config, false).await.unwrap();

        worker.join().unwrap();
        assert_eq!(snapshot.mode, SnapshotMode::Partial);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn exact_nuget_package_is_live() {
        assert_exact_package(true).await;
    }

    #[tokio::test]
    async fn lookalike_nuget_package_is_missing() {
        assert_exact_package(false).await;
    }

    async fn assert_exact_package(exact: bool) {
        let index = json!({
            "resources": [{
                "@type": "SearchQueryService/3.5.0",
                "@id": "FIXTURE_ENDPOINT/query"
            }]
        })
        .to_string();

        let id = if exact {
            "Example.Caching"
        } else {
            "Different.Package"
        };

        let response =
            json!({"data":[{"id":id,"version":"10.3.0","totalDownloads":42}]}).to_string();

        let (endpoint, requests, worker) =
            fixture(vec![(200, account()), (200, index), (200, response)]);

        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint.clone();
        collector.nuget_api = endpoint;
        let mut config = config();
        config.publications = full_config().publications;

        let snapshot = collector.collect(&config, false).await.unwrap();

        worker.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert_eq!(
            snapshot.mode,
            if exact {
                SnapshotMode::Live
            } else {
                SnapshotMode::Partial
            }
        );
        assert_eq!(
            snapshot.packages[0].version.as_deref(),
            if exact { Some("10.3.0") } else { None }
        );
    }

    #[tokio::test]
    async fn owner_discovery_paginates_includes_prereleases_and_filters_lookalikes() {
        let index =
            json!({"resources":[{"@type":"SearchQueryService","@id":"FIXTURE_ENDPOINT/query"}]})
                .to_string();

        let first = json!({"totalHits":3,"data":[
            {"id":"Example.Migrations.SqlServer","owners":["EXAMPLE-LABS"],"version":"10.0.0-rc.1","totalDownloads":1},
            {"id":"Example.MigrationsEvil","owners":["example-labs"],"version":"1.0.0","totalDownloads":1}
        ]}).to_string();

        let second = json!({"totalHits":3,"data":[{"id":"Example.Migrations.Other","owners":"someone-else","version":"1.0.0","totalDownloads":1}]}).to_string();
        let (endpoint, requests, worker) = fixture(vec![(200, index), (200, first), (200, second)]);
        let mut collector = Collector::new(Some("private-github-token".into())).unwrap();
        collector.nuget_api = endpoint;
        let mut config = full_config();
        config
            .publications
            .iter_mut()
            .find(|group| group.id == "safe-migrations")
            .unwrap()
            .discovery_prefixes = vec!["Example.Migrations".into()];

        let packages = collector
            .discover_packages(&config, "example-labs", None)
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].version.as_deref(), Some("10.0.0-rc.1"));
        let requests = requests.lock().unwrap();
        assert!(requests[1].contains("prerelease=true"));
        assert!(requests[2].contains("skip=2"));
        assert!(
            requests
                .iter()
                .all(|request| !request.contains("private-github-token"))
        );
    }

    #[tokio::test]
    async fn changing_inventory_total_is_rejected() {
        let index =
            json!({"resources":[{"@type":"SearchQueryService","@id":"FIXTURE_ENDPOINT/query"}]})
                .to_string();

        let first = json!({"totalHits":2,"data":[{"id":"unrelated.one","version":"1","owners":[],"totalDownloads":0}]}).to_string();
        let second = json!({"totalHits":1,"data":[{"id":"unrelated.two","version":"1","owners":[],"totalDownloads":0}]}).to_string();
        let (endpoint, _, worker) = fixture(vec![(200, index), (200, first), (200, second)]);
        let mut collector = Collector::new(None).unwrap();
        collector.nuget_api = endpoint;

        let result = collector
            .discover_packages(&config(), "example-labs", None)
            .await;

        worker.join().unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn incomplete_inventory_is_rejected() {
        assert_invalid_inventory(json!({"totalHits":1,"data":[]})).await;
    }

    #[tokio::test]
    async fn oversized_inventory_is_rejected() {
        assert_invalid_inventory(json!({"totalHits":1001,"data":[]})).await;
    }

    #[tokio::test]
    async fn missing_inventory_total_is_rejected() {
        assert_invalid_inventory(json!({"data":[]})).await;
    }

    #[tokio::test]
    async fn unsafe_package_id_is_rejected() {
        assert_invalid_inventory(json!({"totalHits":1,"data":[{"id":"Example.Migrations.X](https://evil.test)","owners":["example-labs"],"version":"1","totalDownloads":0}]})).await;
    }

    #[tokio::test]
    async fn empty_package_version_is_rejected() {
        assert_invalid_inventory(json!({"totalHits":1,"data":[{"id":"Example.Migrations.SqlServer","owners":["example-labs"],"version":""}]})).await;
    }

    #[tokio::test]
    async fn duplicate_inventory_entry_is_rejected() {
        assert_invalid_inventory(json!({"totalHits":2,"data":[{"id":"Example.Migrations.SqlServer","owners":["example-labs"],"version":"1","totalDownloads":0},{"id":"Example.Migrations.SqlServer","owners":["example-labs"],"version":"1","totalDownloads":0}]})).await;
    }

    async fn assert_invalid_inventory(response: Value) {
        let index =
            json!({"resources":[{"@type":"SearchQueryService","@id":"FIXTURE_ENDPOINT/query"}]})
                .to_string();

        let (endpoint, _, worker) = fixture(vec![(200, index), (200, response.to_string())]);
        let mut collector = Collector::new(None).unwrap();
        collector.nuget_api = endpoint;
        let mut config = full_config();
        config
            .publications
            .iter_mut()
            .find(|group| group.id == "safe-migrations")
            .unwrap()
            .discovery_prefixes = vec!["Example.Migrations".into()];

        let result = collector
            .discover_packages(&config, "example-labs", None)
            .await;

        worker.join().unwrap();
        assert!(result.is_err());
    }

    fn contribution_body() -> String {
        let collection = json!({
            "totalCommitContributions": 1,
            "totalIssueContributions": 0,
            "totalPullRequestContributions": 0,
            "totalPullRequestReviewContributions": 0,
            "restrictedContributionsCount": 0,
            "contributionCalendar": {
                "totalContributions": 1,
                "weeks": [
                    {
                        "contributionDays": [
                            {
                                "date": "2026-09-05",
                                "contributionCount": 1,
                                "contributionLevel": "FIRST_QUARTILE"
                            }
                        ]
                    }
                ]
            }
        });

        json!({"data":{"user":{"contributionsCollection":collection}}}).to_string()
    }

    #[tokio::test]
    async fn valid_contribution() {
        assert_contribution(contribution_body(), true).await;
    }

    #[tokio::test]
    async fn invalid_contribution_date() {
        assert_contribution(
            contribution_body().replace("2026-09-05", "invalid-date"),
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn invalid_contribution_level() {
        assert_contribution(
            contribution_body().replace("FIRST_QUARTILE", "UNKNOWN"),
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn graphql_error_redacted() {
        assert_contribution(
            json!({"errors":[{"message":"secret-payload"}]}).to_string(),
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn missing_contribution_counters() {
        assert_contribution(
            json!({"data":{"user":{"contributionsCollection":{}}}}).to_string(),
            false,
        )
        .await;
    }

    async fn assert_contribution(body: String, valid: bool) {
        let (endpoint, _, worker) = fixture(vec![(200, body)]);
        let mut collector = Collector::new(Some("token".into())).unwrap();
        collector.graphql_api = endpoint;

        let result = collector.fetch_contributions("owner").await;

        worker.join().unwrap();
        assert_eq!(result.is_ok(), valid);
        if let Err(error) = result {
            assert!(!format!("{error:?}").contains("secret-payload"));
        }
    }

    #[tokio::test]
    async fn missing_user_counter_is_not_reported_as_zero() {
        let mut response: Value = serde_json::from_str(&account()).unwrap();
        response.as_object_mut().unwrap().remove("followers");
        let (endpoint, _, worker) = fixture(vec![(200, response.to_string())]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;

        let result = collector.collect(&config(), false).await;

        worker.join().unwrap();
        assert!(result.is_err());
    }
    #[tokio::test]
    async fn owner_cannot_admit_another_organizations_package_family() {
        let index =
            json!({"resources":[{"@type":"SearchQueryService","@id":"FIXTURE_ENDPOINT/query"}]})
                .to_string();

        let response = json!({"totalHits":1,"data":[{"id":"Example.Migrations.Core","owners":["second-labs"],"version":"1.0.0","totalDownloads":3}]}).to_string();
        let (endpoint, _, worker) = fixture(vec![(200, index), (200, response)]);
        let mut collector = Collector::new(None).unwrap();
        collector.nuget_api = endpoint;
        let config = full_config();

        let result = collector
            .discover_packages(&config, "second-labs", None)
            .await
            .unwrap();

        worker.join().unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn group_owner_overrides_legacy_single_owner() {
        let index =
            json!({"resources":[{"@type":"SearchQueryService","@id":"FIXTURE_ENDPOINT/query"}]})
                .to_string();

        let response = json!({"totalHits":1,"data":[{"id":"Example.Migrations.Core","owners":["second-labs"],"version":"1.0.0","totalDownloads":3}]}).to_string();
        let (endpoint, _, worker) = fixture(vec![(200, index), (200, response)]);
        let mut collector = Collector::new(None).unwrap();
        collector.nuget_api = endpoint;
        let mut config = full_config();
        config.publications[0].owner = Some("second-labs".into());

        let result = collector
            .discover_packages(&config, "second-labs", None)
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].owner.as_deref(), Some("second-labs"));
    }

    #[tokio::test]
    async fn configured_package_with_wrong_owner_is_not_accepted() {
        let response = json!({"data":[{"id":"Example.Caching","owners":["other-owner"],"version":"1.0.0","totalDownloads":3}]}).to_string();
        let (endpoint, _, worker) = fixture(vec![(200, response)]);
        let collector = Collector::new(None).unwrap();
        let mut warnings = Vec::new();

        let result = collector
            .collect_packages(&full_config(), &mut warnings, &endpoint)
            .await;

        worker.join().unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].version.is_none());
        assert!(!warnings.is_empty());
    }

    #[tokio::test]
    async fn effective_private_authorization_does_not_require_a_second_config_opt_in() {
        let (endpoint, requests, worker) = fixture(vec![
            (200, account()),
            (
                200,
                json!({"data":{"user":{"repositories":{"totalCount":7}}}}).to_string(),
            ),
        ]);
        let mut collector = Collector::new(None)
            .unwrap()
            .with_private_token(Some("private-token".into()));

        collector.github_api = endpoint.clone();
        collector.graphql_api = endpoint;
        let mut config = config();
        config.collection.collect_private_repository_count = false;

        let result = collector.collect(&config, true).await.unwrap();

        worker.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 2);
        assert_eq!(result.private_repository_count, Some(7));
    }

    #[tokio::test]
    async fn oversized_http_body_is_rejected_before_decoding() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let Some(mut stream) = accept_fixture(&listener) else {
                return;
            };
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_RESPONSE_BYTES + 1
            )
            .unwrap();
        });

        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;

        let result = collector.fetch_account("owner", AccountKind::User).await;

        worker.join().unwrap();
        assert!(result.unwrap_err().to_string().contains("size limit"));
    }

    #[tokio::test]
    async fn repository_page_cannot_introduce_an_unselected_owner() {
        let response = json!([{"owner":{"login":"different"},"name":"project","full_name":"different/project","html_url":"https://github.com/different/project","description":null,"language":null,"pushed_at":null}]).to_string();
        let (endpoint, _, worker) = fixture(vec![(200, response)]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;

        let result = collector
            .list_repositories("owner", AccountKind::User, 100)
            .await;

        worker.join().unwrap();
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid public repository page")
        );
    }
    fn repository_page(count: usize) -> String {
        serde_json::to_string(&(0..count).map(|index| json!({
            "owner":{"login":"owner"}, "name":format!("project-{index}"),
            "full_name":format!("owner/project-{index}"), "html_url":format!("https://github.com/owner/project-{index}"),
            "description":null, "language":null, "pushed_at":null
        })).collect::<Vec<_>>()).unwrap()
    }

    #[tokio::test]
    async fn configured_repository_limit_marks_collected_snapshot_incomplete() {
        let (endpoint, _, worker) = fixture(vec![(200, account()), (200, repository_page(2))]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;
        let mut config = config();
        config.collection.repository_limit = 1;
        config.collection.discover_public_repositories = true;

        let snapshot = collector.collect(&config, false).await.unwrap();

        worker.join().unwrap();
        assert_eq!(snapshot.repositories.len(), 1);
        assert_eq!(snapshot.mode, SnapshotMode::Partial);
        assert!(
            snapshot
                .sources
                .iter()
                .any(|source| source.source == "github:repositories:owner"
                    && source.status == DataStatus::Missing)
        );
    }

    #[tokio::test]
    async fn exhausted_repository_page_is_complete() {
        let (endpoint, _, worker) = fixture(vec![(200, repository_page(2))]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;

        let inventory = collector
            .list_repositories("owner", AccountKind::User, 3)
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(inventory.repositories.len(), 2);
        assert!(inventory.complete);
    }

    #[tokio::test]
    async fn full_page_at_repository_limit_cannot_claim_exhaustion() {
        let (endpoint, _, worker) = fixture(vec![(200, repository_page(100))]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;

        let inventory = collector
            .list_repositories("owner", AccountKind::User, 100)
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(inventory.repositories.len(), 100);
        assert!(!inventory.complete);
    }

    #[tokio::test]
    async fn absent_effective_authorization_never_requests_private_count() {
        let (endpoint, requests, worker) = fixture(vec![(200, account())]);
        let mut collector = Collector::new(None)
            .unwrap()
            .with_private_token(Some("private-token".into()));
        collector.github_api = endpoint.clone();
        collector.graphql_api = endpoint;
        let mut config = config();
        config.collection.collect_private_repository_count = true;

        let snapshot = collector.collect(&config, false).await.unwrap();

        worker.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert!(snapshot.private_repository_count.is_none());
    }

    fn test_retry_policy() -> RetryPolicy {
        RetryPolicy {
            budget: std::time::Duration::from_secs(2),
            initial_delay: std::time::Duration::from_millis(1),
            attempts: 3,
        }
    }

    #[tokio::test]
    async fn transient_response_retries_same_origin_and_credentials() {
        let (endpoint, requests, worker) =
            fixture(vec![(503, "temporary".into()), (200, account())]);
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let request = client
            .get(&endpoint)
            .header(AUTHORIZATION, "Bearer isolated-test-token");

        let response = send_with_policy(request, test_retry_policy())
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
    }

    #[tokio::test]
    async fn transient_retries_stop_at_attempt_bound() {
        let (endpoint, requests, worker) = fixture(vec![
            (503, "temporary".into()),
            (503, "temporary".into()),
            (503, "temporary".into()),
        ]);
        let request = Client::new().get(&endpoint);

        let response = send_with_policy(request, test_retry_policy())
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn ordinary_forbidden_response_is_not_retried() {
        let (endpoint, requests, worker) = fixture(vec![(403, "private-error".into())]);
        let request = Client::new().get(&endpoint);

        let response = send_with_policy(request, test_retry_policy())
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn retry_after_longer_than_budget_is_never_shortened() {
        let (endpoint, requests, worker) =
            fixture_with_headers(vec![(429, "rate-limit".into())], "Retry-After: 60\r\n");
        let request = Client::new().get(&endpoint);

        let response = send_with_policy(request, test_retry_policy())
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn primary_limit_uses_later_reset_instead_of_shorter_retry_after() {
        let now = 1_700_000_000;
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("1"));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        headers.insert(
            "x-ratelimit-reset",
            HeaderValue::from_str(&(now + 120).to_string()).unwrap(),
        );

        let delay = retry_delay_at(
            StatusCode::FORBIDDEN,
            &headers,
            std::time::Duration::from_millis(1),
            now,
        );

        assert_eq!(delay, Some(std::time::Duration::from_secs(120)));
    }

    #[test]
    fn http_date_retry_after_is_honored() {
        let now = 1_700_000_000;
        let retry_at = chrono::DateTime::from_timestamp(now + 120, 0)
            .unwrap()
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_str(&retry_at).unwrap());

        let delay = retry_delay_at(
            StatusCode::SERVICE_UNAVAILABLE,
            &headers,
            std::time::Duration::from_millis(1),
            now,
        );

        assert_eq!(delay, Some(std::time::Duration::from_secs(120)));
    }

    #[test]
    fn secondary_limit_without_headers_waits_at_least_one_minute() {
        let delay = retry_delay(
            StatusCode::TOO_MANY_REQUESTS,
            &HeaderMap::new(),
            std::time::Duration::from_millis(1),
        );

        assert_eq!(delay, Some(std::time::Duration::from_secs(60)));
    }

    #[test]
    fn malformed_retry_after_stops_instead_of_guessing_an_earlier_delay() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("invalid"));

        let delay = retry_delay(
            StatusCode::SERVICE_UNAVAILABLE,
            &headers,
            std::time::Duration::from_millis(1),
        );

        assert_eq!(delay, None);
    }
    #[tokio::test]
    async fn malformed_success_body_is_not_retried() {
        let (endpoint, requests, worker) = fixture(vec![(200, "not-json".into())]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;

        let result = collector.fetch_account("owner", AccountKind::User).await;

        worker.join().unwrap();
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid response JSON")
        );
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn network_failure_exhaustion_retains_only_redacted_error_category() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/private-path", listener.local_addr().unwrap());
        drop(listener);
        let request = Client::new()
            .get(&endpoint)
            .header(AUTHORIZATION, "Bearer never-log-token");

        let result = send_with_policy(request, test_retry_policy()).await;

        assert_eq!(
            result.unwrap_err().to_string(),
            "HTTP transport or decoding failed"
        );
    }

    #[tokio::test]
    async fn pagination_page_bound_cannot_claim_complete_inventory() {
        let responses = (0..10).map(|_| (200, repository_page(100))).collect();
        let (endpoint, requests, worker) = fixture(responses);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;

        let inventory = collector
            .list_repositories("owner", AccountKind::User, 1000)
            .await
            .unwrap();

        worker.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 10);
        assert_eq!(inventory.repositories.len(), 1000);
        assert!(!inventory.complete);
    }

    #[test]
    fn later_attempt_receives_only_the_unspent_shared_budget() {
        let start = tokio::time::Instant::now();
        let deadline = start + std::time::Duration::from_secs(24);
        let after_previous_attempt = start + std::time::Duration::from_secs(17);
        let original = Client::new().get("https://api.github.com");

        let request = request_with_remaining_budget(&original, deadline, after_previous_attempt)
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            request.timeout().copied(),
            Some(std::time::Duration::from_secs(7))
        );
    }

    #[test]
    fn expired_shared_budget_prevents_another_request() {
        let deadline = tokio::time::Instant::now();
        let original = Client::new().get("https://api.github.com");

        let result = request_with_remaining_budget(&original, deadline, deadline);

        assert!(matches!(result, Err(CollectorError::Client)));
    }

    #[tokio::test]
    async fn response_body_keeps_the_remaining_deadline_after_a_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (release, held_body) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let Some(mut first) = accept_fixture(&listener) else {
                return;
            };
            let mut request = [0; 4096];
            let _ = first.read(&mut request).unwrap();
            write!(
                first,
                "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            drop(first);
            let Some(mut second) = accept_fixture(&listener) else {
                return;
            };
            let _ = second.read(&mut request).unwrap();
            write!(
                second,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            second.flush().unwrap();

            // Hold the body until the assertion's read completes; no competing timed body delivery.
            let _ = held_body.recv_timeout(std::time::Duration::from_secs(5));
        });
        let request = Client::new().get(&endpoint);
        let policy = RetryPolicy {
            budget: std::time::Duration::from_secs(3),
            initial_delay: std::time::Duration::from_millis(1),
            attempts: 3,
        };

        let response = send_with_policy(request, policy).await;
        let body_timed_out = match response {
            Ok(response) => response
                .bytes()
                .await
                .is_err_and(|error| error.is_timeout()),
            Err(_) => false,
        };

        let _ = release.send(());
        worker.join().unwrap();
        assert!(
            body_timed_out,
            "response headers must succeed and the withheld body must time out"
        );
    }
    #[tokio::test]
    async fn scoped_organization_repository_failure_keeps_its_selected_cause() {
        let (endpoint, _, worker) = fixture(vec![
            (200, account()),
            (403, "SECRET remote failure".into()),
        ]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint;
        let mut config = config();
        config.profile.variant = sourcefield_core::ProfileVariant::Organization;
        config.collection.github_user.clear();
        config.collection.github_organizations = vec!["example-labs".into()];
        config.collection.discover_public_repositories = true;

        let snapshot = collector.collect(&config, false).await.unwrap();
        let scoped = sourcefield_core::scope_snapshot(&config, &snapshot);

        worker.join().unwrap();
        assert_eq!(scoped.mode, SnapshotMode::Partial);
        assert!(
            scoped
                .sources
                .iter()
                .any(|source| source.source == "github:repositories:example-labs"
                    && source.status == DataStatus::Missing)
        );
        assert!(
            !scoped
                .sources
                .iter()
                .any(|source| source.source == "github:repositories")
        );
        assert!(scoped.warnings.is_empty());
    }

    #[tokio::test]
    async fn scoped_nuget_discovery_failure_keeps_service_and_authored_package_causes() {
        let (endpoint, _, worker) = fixture(vec![
            (200, account()),
            (404, "SECRET registry failure".into()),
        ]);
        let mut collector = Collector::new(None).unwrap();
        collector.github_api = endpoint.clone();
        collector.nuget_api = endpoint;
        let mut config = config();
        config.profile.variant = sourcefield_core::ProfileVariant::Organization;
        config.collection.github_user.clear();
        config.collection.github_organizations = vec!["example-labs".into()];
        config.publications = full_config().publications;
        config.collection.nuget_owner = Some("example-labs".into());

        let snapshot = collector.collect(&config, false).await.unwrap();
        let scoped = sourcefield_core::scope_snapshot(&config, &snapshot);

        worker.join().unwrap();
        assert_eq!(scoped.mode, SnapshotMode::Partial);
        assert!(
            scoped
                .sources
                .iter()
                .any(|source| source.source == "nuget:service-index"
                    && source.status == DataStatus::Missing)
        );
        assert!(
            scoped
                .sources
                .iter()
                .any(|source| source.source == "nuget:example.caching"
                    && source.status == DataStatus::Missing)
        );
        assert!(scoped.warnings.is_empty());
    }
}
