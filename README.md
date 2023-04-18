# pg_human

A Postgres extension that uses GPT to humanize postgres. Currently it can take a
command in natural language and create and executed a Postgres query for it.

**IMPORTANT**: If the AI has a bad day it might tell Postgres to delete all your
data. Use at your own risk. Using it on a production database is definitely not
advised.

## How to set this up

One time setup of `cargo pgx`
```bash
cargo install --locked cargo-pgx
cargo pgx init
```

Run Postgres:
```bash
cargo pgx run
```

Install the extension:
```sql
CREATE EXTENSION pg_human;
```

Configure your OpenAI API key:
```sql
ALTER SYSTEM SET pg_human.api_key TO 'key here';
SELECT pg_reload_conf();
```

If you're using Azure OpenAI Service you should set a few more variables:
```sql
ALTER SYSTEM SET pg_human.api_type = 'Azure';
ALTER SYSTEM SET pg_human.base_url = 'https://{resource-name-here}.openai.azure.com/openai/deployments/{deployment-name-here}/';
```

## How to play with this

Only show a query that you can manually copy paste before executing it using
`give_me_a_query_to()`:
```sql
SELECT give_me_a_query_to('create tables for a todo app with multiple u
sers');
NOTICE:  00000: You can try this query:
Here's the code:

\`\`\`
CREATE TABLE public.users (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT UNIQUE NOT NULL,
    password TEXT NOT NULL
);

CREATE TABLE public.tasks (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES public.users(id),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW()
);
\`\`\`
DETAIL:
LOCATION:  pg_human::give_me_a_query_to::{{closure}}, lib.rs:255
```


```sql
SELECT data FROM im_feeling_lucky('show the amount of completed tasks by user');
NOTICE:  00000: Executing query:
SELECT users.name, COUNT(tasks.completed) AS completed_tasks
FROM public.users
LEFT JOIN public.tasks ON users.id = tasks.user_id AND tasks.completed = true
GROUP BY users.id;
DETAIL:
LOCATION:  pg_human::im_feeling_lucky::{{closure}}, lib.rs:270
                        data
─────────────────────────────────────────────────────
 {"name": "Cristiano Ronaldo", "completed_tasks": 1}
 {"name": "Neymar Jr.", "completed_tasks": 2}
 {"name": "Lionel Messi", "completed_tasks": 1}
(3 rows)
```

```sql
SELECT im_feeling_very_lucky('add 3 famous football players and 3 tasks ea
ch based on their training schedule');
NOTICE:  00000: Executing:
INSERT INTO public.users (name, email, password)
VALUES
('Lionel Messi', 'messi@example.com', 'password1'),
('Cristiano Ronaldo', 'ronaldo@example.com', 'password2'),
('Neymar Jr.', 'neymar@example.com', 'password3');

INSERT INTO public.tasks (user_id, title, description, completed, created_at)
VALUES
(1, 'Morning training session', 'Running and stretching for 1 hour', false, now()),
(1, 'Afternoon gym session', 'Weight training and cardio for 1.5 hours', false, now()),
(1, 'Evening pool session', 'Swimming drills and relaxation for 1 hour', false, now()),
(2, 'Morning gym session', 'Weight training and cardio for 1.5 hours', false, now()),
(2, 'Afternoon skills training', 'Ball control drills and practice of shooting skills for 1.5 hours', false, now()),
(2, 'Evening pool session', 'Swimming drills and relaxation for 1 hour', false, now()),
(3, 'Morning running session', 'Running drills and stamina training for 1 hour', false, now()),
(3, 'Afternoon skills and tactics session', 'Ball control drills, team strategy, and practice of set plays for 1.5 hours', false, now()),
(3, 'Evening pool session', 'Swimming drills and relaxation for 1 hour', false, now());
```

## Contributing

At the current stage it's not a very serious project, it's a toy example for my
2023 CitusCon talk.  If you think this is cool and want to add to it, feel free
to build on top of it. But for now I don't plan to maintain this or to spend
lots of time reviewing non-trivial PRs.
