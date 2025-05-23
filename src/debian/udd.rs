use crate::dependencies::debian::DebianDependency;
use crate::dependencies::debian::TieBreaker;
use sqlx::{Error, PgPool};
use tokio::runtime::Runtime;

/// Connection to the Ultimate Debian Database (UDD).
///
/// UDD is a central Debian database that combines data from various
/// Debian sources, such as the archive, the BTS, popcon, etc.
pub struct UDD {
    /// Database connection pool
    pool: PgPool,
}

impl UDD {
    // Function to create a new instance of UDD with a database connection
    /// Connect to the UDD database.
    ///
    /// # Returns
    /// A new UDD instance connected to the database, or an error if the connection fails
    pub async fn connect() -> Result<Self, Error> {
        let pool =
            PgPool::connect("postgres://udd-mirror:udd-mirror@udd-mirror.debian.net:5432/udd")
                .await
                .unwrap();
        Ok(UDD { pool })
    }
}

/// Find the most popular package from a list of dependencies according to popcon.
///
/// # Arguments
/// * `reqs` - List of Debian dependencies to choose from
///
/// # Returns
/// The name of the most popular package, or None if no package is found in popcon
async fn get_most_popular(reqs: &[&DebianDependency]) -> Result<Option<String>, Error> {
    let udd = UDD::connect().await.unwrap();
    let names = reqs
        .iter()
        .flat_map(|req| req.package_names())
        .collect::<Vec<_>>();

    let (max_popcon_name,): (Option<String>,) = sqlx::query_as(
        "SELECT package FROM popcon WHERE package IN $1 ORDER BY insts DESC LIMIT 1",
    )
    .bind(names)
    .fetch_one(&udd.pool)
    .await
    .unwrap();

    Ok(max_popcon_name)
}

/// Tie-breaker that selects dependencies based on popcon popularity.
///
/// This tie-breaker uses the Debian Popularity Contest (popcon) statistics
/// to determine which package is most commonly installed among Debian users.
pub struct PopconTieBreaker;

impl TieBreaker for PopconTieBreaker {
    fn break_tie<'a>(&self, reqs: &[&'a DebianDependency]) -> Option<&'a DebianDependency> {
        // TODO(jelmer): Pick package based on what appears most commonly in
        // build-depends{-indep,-arch}
        let rt = Runtime::new().unwrap();
        let package = rt.block_on(get_most_popular(reqs)).unwrap();
        if package.is_none() {
            log::info!("No relevant popcon information found, not ranking by popcon");
            return None;
        }
        let package = package.unwrap();
        let winner = reqs
            .into_iter()
            .find(|req| req.package_names().contains(&package.to_string()));

        if winner.is_none() {
            log::info!("No relevant popcon information found, not ranking by popcon");
        }

        winner.copied()
    }
}
