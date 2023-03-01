use itertools::Itertools;
use pgx::pg_sys;
use pgx::prelude::*;
use std::ffi::{CStr, CString};
use std::fmt;

pgx::pg_module_magic!();

extension_sql!(
    r#"
CREATE TABLE companies (
  id bigserial PRIMARY KEY,
  name text NOT NULL,
  image_url text,
  created_at timestamp without time zone NOT NULL,
  updated_at timestamp without time zone NOT NULL
);

CREATE TABLE campaigns (
  id bigserial,
  company_id bigint REFERENCES companies (id),
  name text NOT NULL,
  cost_model text NOT NULL,
  state text NOT NULL,
  monthly_budget bigint,
  blacklisted_site_urls text[],
  created_at timestamp without time zone NOT NULL,
  updated_at timestamp without time zone NOT NULL,
  PRIMARY KEY (company_id, id)
);

CREATE TABLE ads (
  id bigserial,
  company_id bigint,
  campaign_id bigint,
  name text NOT NULL,
  image_url text,
  target_url text,
  impressions_count bigint DEFAULT 0,
  clicks_count bigint DEFAULT 0,
  created_at timestamp without time zone NOT NULL,
  updated_at timestamp without time zone NOT NULL,
  PRIMARY KEY (company_id, id),
  FOREIGN KEY (company_id, campaign_id)
    REFERENCES campaigns (company_id, id)
);

CREATE TABLE clicks (
  id bigserial,
  company_id bigint,
  ad_id bigint,
  clicked_at timestamp without time zone NOT NULL,
  site_url text NOT NULL,
  cost_per_click_usd numeric(20,10),
  user_ip inet NOT NULL,
  user_data jsonb NOT NULL,
  PRIMARY KEY (company_id, id),
  FOREIGN KEY (company_id, ad_id)
    REFERENCES ads (company_id, id)
);

CREATE TABLE impressions (
  id bigserial,         -- was: PRIMARY KEY
  company_id bigint,    -- added
  ad_id bigint,         -- was: REFERENCES ads (id),
  seen_at timestamp without time zone NOT NULL,
  site_url text NOT NULL,
  cost_per_impression_usd numeric(20,10),
  user_ip inet NOT NULL,
  user_data jsonb NOT NULL,
  PRIMARY KEY (company_id, id),
  FOREIGN KEY (company_id, ad_id)
    REFERENCES ads (company_id, id)
);
"#,
    name = "example_schema",
);

#[derive(Debug)]
struct DatabaseDescription {
    tables: Vec<TableDescription>,
}

impl fmt::Display for DatabaseDescription {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        for (i, table) in self.tables.iter().enumerate() {
            if formatter.alternate() {
                if i > 0 {
                    write!(formatter, "\n\n")?
                }
                write!(formatter, "{:#}", table)?
            } else {
                write!(formatter, "{}", table)?
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct TableDescription {
    schema: String,
    name: String,
    columns: Vec<ColumnDescription>,
}

fn quote_qualified_identifier<StringLike: AsRef<str>>(
    qualifier: StringLike,
    ident: StringLike,
) -> String {
    let quoted_cstr = unsafe {
        let qualifier_cstr = CString::new(qualifier.as_ref()).unwrap();
        let ident_cstr = CString::new(ident.as_ref()).unwrap();
        let quoted_ptr =
            pg_sys::quote_qualified_identifier(qualifier_cstr.as_ptr(), ident_cstr.as_ptr());
        CStr::from_ptr(quoted_ptr)
    };
    quoted_cstr.to_str().unwrap().to_string()
}

impl fmt::Display for TableDescription {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "CREATE TABLE {}(",
            quote_qualified_identifier(&self.schema, &self.name)
        )?;
        for (i, column) in self.columns.iter().enumerate() {
            if i > 0 {
                write!(formatter, ",")?
            }
            if formatter.alternate() {
                write!(formatter, "\n{:#}", column)?
            } else {
                if i > 0 {
                    write!(formatter, " ")?
                }
                write!(formatter, "{}", column)?
            }
        }
        write!(formatter, ");")?;
        Ok(())
    }
}

#[derive(Debug)]
struct ColumnDescription {
    name: String,
    type_name: String,
}

impl fmt::Display for ColumnDescription {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{} {}", self.name, self.type_name)
    }
}

#[pg_extern]
fn gpt_feeling_lucky() -> Result<String, reqwest::Error> {
    let query = r#"
    SELECT
        table_schema::text,
        table_name::text,
        column_name::text,
        data_type::text
    FROM information_schema.columns
    WHERE table_schema = ANY(current_schemas(false))
    ORDER BY table_schema, table_name, ordinal_position;
    "#;
    let tables: Vec<_> = Spi::connect(|client| {
        client
            .select(query, None, None)
            .unwrap()
            .group_by(|row| {
                (
                    row[1].value::<String>().unwrap().unwrap(),
                    row[2].value::<String>().unwrap().unwrap(),
                )
            })
            .into_iter()
            .map(|(key, group)| TableDescription {
                schema: key.0,
                name: key.1,
                columns: group
                    .map(|row| ColumnDescription {
                        name: row[3].value::<String>().unwrap().unwrap(),
                        type_name: row[4].value::<String>().unwrap().unwrap(),
                    })
                    .collect(),
            })
            .collect()
    });
    // TODO: Add primary keys and foreign keys
    let db_description = DatabaseDescription { tables };
    Ok(format!("{}", db_description))
    // reqwest::blocking::get("https://api.ipify.org")?.text()
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgx::prelude::*;

    #[pg_test]
    fn test_hello_pg_gpt() {
        assert_eq!("Hello, pg_gpt", crate::gpt_feeling_lucky().unwrap());
    }
}

/// This module is required by `cargo pgx test` invocations.
/// It must be visible at the root of your extension crate.
#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {
        // perform one-off initialization when the pg_test framework starts
    }

    pub fn postgresql_conf_options() -> Vec<&'static str> {
        // return any postgresql.conf settings that are required for your tests
        vec![]
    }
}
