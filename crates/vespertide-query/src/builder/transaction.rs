use super::PlanQueries;
use crate::DatabaseBackend;
use crate::sql::{BuiltQuery, RawSql};

pub(super) fn wrap_backend_queries(plan_queries: &mut [PlanQueries], backend: DatabaseBackend) {
    let Some(first_idx) = plan_queries
        .iter()
        .position(|pq| !backend_queries(pq, backend).is_empty())
    else {
        return;
    };
    let Some(last_idx) = plan_queries
        .iter()
        .rposition(|pq| !backend_queries(pq, backend).is_empty())
    else {
        return;
    };

    backend_queries_mut(&mut plan_queries[first_idx], backend)
        .insert(0, BuiltQuery::Raw(RawSql::uniform("BEGIN;".to_string())));
    backend_queries_mut(&mut plan_queries[last_idx], backend)
        .push(BuiltQuery::Raw(RawSql::uniform("COMMIT;".to_string())));
}

fn backend_queries(plan_queries: &PlanQueries, backend: DatabaseBackend) -> &[BuiltQuery] {
    match backend {
        DatabaseBackend::Postgres => &plan_queries.postgres,
        DatabaseBackend::MySql => &plan_queries.mysql,
        DatabaseBackend::Sqlite => &plan_queries.sqlite,
    }
}

fn backend_queries_mut(
    plan_queries: &mut PlanQueries,
    backend: DatabaseBackend,
) -> &mut Vec<BuiltQuery> {
    match backend {
        DatabaseBackend::Postgres => &mut plan_queries.postgres,
        DatabaseBackend::MySql => &mut plan_queries.mysql,
        DatabaseBackend::Sqlite => &mut plan_queries.sqlite,
    }
}
