use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::query_source::QueryableByName;
use diesel::row::NamedRow;
use semver;
use std::error::Error;

use git;
use krate::Crate;
use schema::*;
use util::{human, CargoResult};
use version::Version;

#[derive(Identifiable, Associations, Debug)]
#[belongs_to(Version)]
#[belongs_to(Crate)]
#[table_name = "dependencies"]
pub struct Dependency {
    pub id: i32,
    pub version_id: i32,
    pub crate_id: i32,
    pub req: semver::VersionReq,
    pub optional: bool,
    pub default_features: bool,
    pub features: Vec<String>,
    pub target: Option<String>,
    pub kind: Kind,
}

#[derive(Debug, QueryableByName)]
pub struct ReverseDependency {
    #[diesel(embed)] dependency: Dependency,
    #[sql_type = "::diesel::types::Integer"] crate_downloads: i32,
    #[sql_type = "::diesel::types::Text"]
    #[column_name(crate_name)]
    name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EncodableDependency {
    pub id: i32,
    pub version_id: i32,
    pub crate_id: String,
    pub req: String,
    pub optional: bool,
    pub default_features: bool,
    pub features: Vec<String>,
    pub target: Option<String>,
    pub kind: Kind,
    pub downloads: i32,
}

#[derive(Copy, Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
#[repr(u32)]
pub enum Kind {
    Normal = 0,
    Build = 1,
    Dev = 2,
    // if you add a kind here, be sure to update `from_row` below.
}

#[derive(Default, Insertable, Debug)]
#[table_name = "dependencies"]
pub struct NewDependency<'a> {
    pub version_id: i32,
    pub crate_id: i32,
    pub req: String,
    pub optional: bool,
    pub default_features: bool,
    pub features: Vec<&'a str>,
    pub target: Option<&'a str>,
    pub kind: i32,
}

impl Dependency {
    // `downloads` need only be specified when generating a reverse dependency
    pub fn encodable(self, crate_name: &str, downloads: Option<i32>) -> EncodableDependency {
        EncodableDependency {
            id: self.id,
            version_id: self.version_id,
            crate_id: crate_name.into(),
            req: self.req.to_string(),
            optional: self.optional,
            default_features: self.default_features,
            features: self.features,
            target: self.target,
            kind: self.kind,
            downloads: downloads.unwrap_or(0),
        }
    }
}

impl ReverseDependency {
    pub fn encodable(self, crate_name: &str) -> EncodableDependency {
        self.dependency
            .encodable(crate_name, Some(self.crate_downloads))
    }
}

pub fn add_dependencies(
    conn: &PgConnection,
    deps: &[::upload::CrateDependency],
    version_id: i32,
) -> CargoResult<Vec<git::Dependency>> {
    use diesel::insert_into;

    let git_and_new_dependencies = deps.iter()
        .map(|dep| {
            let krate = Crate::by_name(&dep.name)
                .first::<Crate>(&*conn)
                .map_err(|_| {
                    human(&format_args!("no known crate named `{}`", &*dep.name))
                })?;
            if dep.version_req == semver::VersionReq::parse("*").unwrap() {
                return Err(human(
                    "wildcard (`*`) dependency constraints are not allowed \
                     on crates.io. See http://doc.crates.io/faq.html#can-\
                     libraries-use--as-a-version-for-their-dependencies for more \
                     information",
                ));
            }
            let features: Vec<_> = dep.features.iter().map(|s| &**s).collect();

            Ok((
                git::Dependency {
                    name: dep.name.to_string(),
                    req: dep.version_req.to_string(),
                    features: features.iter().map(|s| s.to_string()).collect(),
                    optional: dep.optional,
                    default_features: dep.default_features,
                    target: dep.target.clone(),
                    kind: dep.kind.or(Some(Kind::Normal)),
                },
                NewDependency {
                    version_id: version_id,
                    crate_id: krate.id,
                    req: dep.version_req.to_string(),
                    kind: dep.kind.unwrap_or(Kind::Normal) as i32,
                    optional: dep.optional,
                    default_features: dep.default_features,
                    features: features,
                    target: dep.target.as_ref().map(|s| &**s),
                },
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let (git_deps, new_dependencies): (Vec<_>, Vec<_>) =
        git_and_new_dependencies.into_iter().unzip();

    insert_into(dependencies::table)
        .values(&new_dependencies)
        .execute(conn)?;

    Ok(git_deps)
}

impl Queryable<dependencies::SqlType, Pg> for Dependency {
    type Row = (
        i32,
        i32,
        i32,
        String,
        bool,
        bool,
        Vec<String>,
        Option<String>,
        i32,
    );

    fn build(row: Self::Row) -> Self {
        Dependency {
            id: row.0,
            version_id: row.1,
            crate_id: row.2,
            req: semver::VersionReq::parse(&row.3).unwrap(),
            optional: row.4,
            default_features: row.5,
            features: row.6,
            target: row.7,
            kind: match row.8 {
                0 => Kind::Normal,
                1 => Kind::Build,
                2 => Kind::Dev,
                n => panic!("unknown kind: {}", n),
            },
        }
    }
}

impl QueryableByName<Pg> for Dependency {
    fn build<R: NamedRow<Pg>>(row: &R) -> Result<Self, Box<Error + Send + Sync>> {
        use schema::dependencies::*;
        use diesel::dsl::SqlTypeOf;

        let req_str = row.get::<SqlTypeOf<req>, String>("req")?;
        Ok(Dependency {
            id: row.get::<SqlTypeOf<id>, _>("id")?,
            version_id: row.get::<SqlTypeOf<version_id>, _>("version_id")?,
            crate_id: row.get::<SqlTypeOf<crate_id>, _>("crate_id")?,
            req: semver::VersionReq::parse(&req_str)?,
            optional: row.get::<SqlTypeOf<optional>, _>("optional")?,
            default_features: row.get::<SqlTypeOf<default_features>, _>("default_features")?,
            features: row.get::<SqlTypeOf<features>, _>("features")?,
            target: row.get::<SqlTypeOf<target>, _>("target")?,
            kind: match row.get::<SqlTypeOf<kind>, _>("kind")? {
                0 => Kind::Normal,
                1 => Kind::Build,
                2 => Kind::Dev,
                n => return Err(format!("unknown kind: {}", n).into()),
            },
        })
    }
}
