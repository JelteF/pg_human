use anyhow::Result;
use itertools::Itertools;
use openai::{
    chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole},
    set_key,
};
use pgx::guc::{GucContext, GucFlags, GucRegistry, GucSetting};
use pgx::prelude::*;
use pgx::spi::quote_qualified_identifier;
use pgx::JsonB;
use std::fmt;
use tokio::time::timeout;
use std::time::Duration;

pgx::pg_module_magic!();

extension_sql_file!("schema.sql");

static API_KEY: GucSetting<Option<&'static str>> = GucSetting::new(None);
#[pg_guard]
pub extern "C" fn _PG_init() {
    GucRegistry::define_string_guc(
        "pg_human.api_key",
        "The OpenAI API key that is used by pg_human",
        "The OpenAI API key that is used by pg_human",
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
                write!(formatter, "{table:#}")?
            } else {
                write!(formatter, "{table}")?
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
    constraints: Vec<String>,
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
                write!(formatter, "\n    {column:#}")?
            } else {
                if i > 0 {
                    write!(formatter, " ")?
                }
                write!(formatter, "{column}")?
            }
        }
        for constraint in self.constraints.iter() {
            write!(formatter, ",")?;
            if formatter.alternate() {
                write!(formatter, "\n    {constraint}")?
            } else {
                write!(formatter, " {constraint}")?
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
        let tables_query = r#"
            SELECT
                table_schema::text,
                table_name::text,
                column_name::text,
                data_type::text
            FROM information_schema.columns
            WHERE table_schema = ANY(current_schemas(false))
            ORDER BY table_schema, table_name, ordinal_position;
            "#;
        let tables = Spi::connect(|client| {
            let mut tables: Vec<_> = client
                .select(tables_query, None, None)
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
                    constraints: vec![],
                })
                .collect();

            // TODO: Use a single query to get all constraints
            let constraint_query = r#"
            SELECT pg_get_constraintdef(con.oid)
               FROM pg_catalog.pg_constraint con
                    INNER JOIN pg_catalog.pg_class rel
                               ON rel.oid = con.conrelid
                    INNER JOIN pg_catalog.pg_namespace nsp
                               ON nsp.oid = connamespace
               WHERE nsp.nspname = $1 AND rel.relname = $2;
            "#;
            for table in tables.iter_mut() {
                let constraints = client
                    .select(
                        constraint_query,
                        None,
                        Some(vec![
                            (
                                PgBuiltInOids::TEXTOID.oid(),
                                table.schema.clone().into_datum(),
                            ),
                            (
                                PgBuiltInOids::TEXTOID.oid(),
                                table.name.clone().into_datum(),
                            ),
                        ]),
                    )
                    .unwrap()
                    .map(|row| row[1].value::<String>().unwrap().unwrap());
                table.constraints.extend(constraints);
            }
            tables
        });
        return DatabaseDescription { tables };
    }
}

#[must_use]
fn question_prompt(question: &str) -> Vec<ChatCompletionMessage> {
    let db_description = DatabaseDescription::new();
    vec![
        ChatCompletionMessage {
            role: ChatCompletionMessageRole::System,
            content: format!("You are a PostgreSQL expert"),
            name: None,
        },
        ChatCompletionMessage {
            role: ChatCompletionMessageRole::User,
            content: format!("My Postgres database schema looks like this:\n{db_description:#}."),
            name: None,
        },
        ChatCompletionMessage {
            role: ChatCompletionMessageRole::User,
            content: format!("Given that schema, could you give me a PostgreSQL query to do the following action: {question}.\n Only respond with the SQL code, so no other additional text. Only use the tables and columns provided in the schema."),
            name: None,
        },
    ]
}

async fn complete_prompt(prompt: Vec<ChatCompletionMessage>) -> Result<String> {
    set_key(API_KEY.get().expect("pg_human.api_key is not set"));
    let request = ChatCompletion::builder("gpt-3.5-turbo", prompt)
        .create();

    // Sometimes the API seems to get stuck, give up after 10 seconds
    // TODO: Use statement timeout instead
    let mut response = timeout(Duration::from_secs(20), request)
        .await???;
    Ok(response.choices.remove(0).message.content)
}

#[pg_extern]
#[tokio::main(flavor = "current_thread")]
async fn give_me_a_query_to(question: &str) -> Result<()> {
    let prompt = question_prompt(question);
    notice!("You can try this query:\n{}", complete_prompt(prompt).await?);
    Ok(())
}

#[pg_extern]
#[tokio::main(flavor = "current_thread")]
async fn im_feeling_lucky(
    question: &str,
) -> Result<TableIterator<'static, (name!(i, i32), name!(data, JsonB))>> {
    let prompt = question_prompt(question);
    let sql = complete_prompt(prompt).await?;
    let cleaned_sql = sql.trim_end_matches([';', '\n', ' ']);
    notice!("Executing query:\n{sql}");
    // let sql = "SELECT 1 as mynumber";
    Spi::connect(|client| {
        let mut results = Vec::new();
        let mut tup_table = client.select(
            &format!(
                "SELECT to_jsonb(generated_query) as data FROM ({cleaned_sql}) generated_query"
            ),
            None,
            None,
        )?;

        let mut i = 0;
        while let Some(row) = tup_table.next() {
            let json_row = row["data"].value::<JsonB>()?.unwrap();
            results.push((i, json_row));
            i += 1;
        }

        Ok(TableIterator::new(results.into_iter()))
    })
}

#[pg_extern]
#[tokio::main(flavor = "current_thread")]
async fn im_feeling_lucky_dml(question: &str) -> Result<()>{
    let prompt = question_prompt(question);
    let sql = complete_prompt(prompt).await?;
    notice!("Executing:\n{sql}");
    Spi::connect(|mut client| {
        client.update(
            &sql,
            None,
            None,
        )?;
        Ok(())
    })
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
        vec!["pg_human.api_key = 'ABC'"]
    }
}
