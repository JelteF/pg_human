use itertools::Itertools;
use pgx::guc::{GucContext, GucFlags, GucRegistry, GucSetting};
use pgx::prelude::*;
use pgx::spi::quote_qualified_identifier;
use std::fmt;
use anyhow::Result;
use openai_api;

pgx::pg_module_magic!();

extension_sql_file!("schema.sql");

static API_KEY: GucSetting<Option<&'static str>> = GucSetting::new(None);
#[pg_guard]
pub extern "C" fn _PG_init() {
    GucRegistry::define_string_guc(
        "pg_gpt.api_key",
        "The OpenAI API key that is used by pg_gpt",
        "The OpenAI API key that is used by pg_gpt",
        &API_KEY,
        GucContext::Userset,
        GucFlags::default(),
    );
}

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
                write!(formatter, "\n    {:#}", column)?
            } else {
                if i > 0 {
                    write!(formatter, " ")?
                }
                write!(formatter, "{}", column)?
            }
        }
        if formatter.alternate() {
            write!(formatter, "\n")?;
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

impl DatabaseDescription {
    #[must_use]
    fn new() -> DatabaseDescription {
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
        return DatabaseDescription { tables };
    }
}

#[must_use]
fn question_prompt(question: &str) -> String {
    let db_description = DatabaseDescription::new();
    format!(
        "Given the following Postgres schema:
{db_description:#}


Could you give me a PostgreSQL query to {question}."
    )
}

#[pg_extern]
#[tokio::main(flavor = "current_thread")]
async fn give_me_a_query_to(question: &str) -> Result<String> {
    let client = openai_api::Client::new(&API_KEY.get().expect("pg_gpt.api_key is not set"));
    let prompt = question_prompt(question);
    let args = openai_api::api::CompletionArgs::builder()
        .prompt(prompt)
        .engine("text-davinci-003")
        .max_tokens(1000);
    let response = client.complete_prompt(args.build()?).await?;
    Ok(response.to_string())
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use super::*;

    #[pg_test]
    fn test_hello_schema_dump() {
        let expected_schema = r#"CREATE TABLE public.ads(
    id bigint,
    company_id bigint,
    campaign_id bigint,
    name text,
    image_url text,
    target_url text,
    impressions_count bigint,
    clicks_count bigint,
    created_at timestamp without time zone,
    updated_at timestamp without time zone
);

CREATE TABLE public.campaigns(
    id bigint,
    company_id bigint,
    name text,
    cost_model text,
    state text,
    monthly_budget bigint,
    blacklisted_site_urls ARRAY,
    created_at timestamp without time zone,
    updated_at timestamp without time zone
);

CREATE TABLE public.clicks(
    id bigint,
    company_id bigint,
    ad_id bigint,
    clicked_at timestamp without time zone,
    site_url text,
    cost_per_click_usd numeric,
    user_ip inet,
    user_data jsonb
);

CREATE TABLE public.companies(
    id bigint,
    name text,
    image_url text,
    created_at timestamp without time zone,
    updated_at timestamp without time zone
);

CREATE TABLE public.impressions(
    id bigint,
    company_id bigint,
    ad_id bigint,
    seen_at timestamp without time zone,
    site_url text,
    cost_per_impression_usd numeric,
    user_ip inet,
    user_data jsonb
);"#;
        assert_eq!(expected_schema, format!("{:#}", DatabaseDescription::new()));
    }

    #[pg_test]
    fn test_guc() {
        assert_eq!(Some("ABC".to_string()), API_KEY.get())
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
        vec!["pg_gpt.api_key = 'ABC'"]
    }
}
