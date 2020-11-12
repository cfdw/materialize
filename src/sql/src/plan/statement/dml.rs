// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! Data manipulation language (DML) statements.

use anyhow::bail;

use ore::collections::CollectionExt;
use std::convert::TryFrom;

use crate::ast::{
    CopyDirection, CopyRelation, CopyStatement, CopyTarget, CreateViewStatement, ExplainStatement,
    Explainee, InsertStatement, SelectStatement, Statement, TailStatement,
};
use crate::catalog::CatalogItemType;
use crate::plan::query::{self, QueryLifetime};
use crate::plan::statement::{CopyOptions, TailOptions};
use crate::plan::{CopyFormat, Params, PeekWhen, Plan, StatementContext};

pub fn handle_explain(
    scx: &StatementContext,
    ExplainStatement {
        stage,
        explainee,
        options,
    }: ExplainStatement,
    params: &Params,
) -> Result<Plan, anyhow::Error> {
    let is_view = matches!(explainee, Explainee::View(_));
    let (scx, query) = match explainee {
        Explainee::View(name) => {
            let full_name = scx.resolve_item(name.clone())?;
            let entry = scx.catalog.get_item(&full_name);
            if entry.item_type() != CatalogItemType::View {
                bail!(
                    "Expected {} to be a view, not a {}",
                    name,
                    entry.item_type(),
                );
            }
            let parsed = crate::parse::parse(entry.create_sql())
                .expect("Sql for existing view should be valid sql");
            let query = match parsed.into_last() {
                Statement::CreateView(CreateViewStatement { query, .. }) => query,
                _ => panic!("Sql for existing view should parse as a view"),
            };
            let scx = StatementContext {
                pcx: entry.plan_cx(),
                catalog: scx.catalog,
                param_types: scx.param_types.clone(),
            };
            (scx, query)
        }
        Explainee::Query(query) => (scx.clone(), query),
    };
    // Previouly we would bail here for ORDER BY and LIMIT; this has been relaxed to silently
    // report the plan without the ORDER BY and LIMIT decorations (which are done in post).
    let (mut sql_expr, desc, finishing) =
        query::plan_root_query(&scx, query, QueryLifetime::OneShot)?;
    let finishing = if is_view {
        // views don't use a separate finishing
        sql_expr.finish(finishing);
        None
    } else if finishing.is_trivial(desc.arity()) {
        None
    } else {
        Some(finishing)
    };
    sql_expr.bind_parameters(&params)?;
    let expr = sql_expr.clone().decorrelate();
    Ok(Plan::ExplainPlan {
        raw_plan: sql_expr,
        decorrelated_plan: expr,
        row_set_finishing: finishing,
        stage,
        options,
    })
}

pub fn handle_select(
    scx: &StatementContext,
    SelectStatement { query, as_of }: SelectStatement,
    params: &Params,
    copy_to: Option<CopyFormat>,
) -> Result<Plan, anyhow::Error> {
    let (mut relation_expr, _desc, finishing) =
        query::plan_root_query(scx, query, QueryLifetime::OneShot)?;
    relation_expr.bind_parameters(&params)?;
    let relation_expr = relation_expr.decorrelate();

    let when = match as_of.map(|e| query::eval_as_of(scx, e)).transpose()? {
        Some(ts) => PeekWhen::AtTimestamp(ts),
        None => PeekWhen::Immediately,
    };

    Ok(Plan::Peek {
        source: relation_expr,
        when,
        finishing,
        copy_to,
    })
}

pub fn handle_tail(
    scx: &StatementContext,
    TailStatement {
        name,
        options,
        as_of,
    }: TailStatement,
    copy_to: Option<CopyFormat>,
) -> Result<Plan, anyhow::Error> {
    let from = scx.resolve_item(name)?;
    let entry = scx.catalog.get_item(&from);
    let ts = as_of.map(|e| query::eval_as_of(scx, e)).transpose()?;
    let options = TailOptions::try_from(options)?;

    match entry.item_type() {
        CatalogItemType::Table | CatalogItemType::Source | CatalogItemType::View => {
            Ok(Plan::Tail {
                id: entry.id(),
                ts,
                with_snapshot: options.snapshot.unwrap_or(true),
                copy_to,
                emit_progress: options.progress.unwrap_or(false),
                object_columns: entry.desc()?.arity(),
            })
        }
        CatalogItemType::Index | CatalogItemType::Sink | CatalogItemType::Type => bail!(
            "'{}' cannot be tailed because it is a {}",
            from,
            entry.item_type(),
        ),
    }
}

pub fn handle_copy(
    scx: &StatementContext,
    CopyStatement {
        relation,
        direction,
        target,
        options,
    }: CopyStatement,
) -> Result<Plan, anyhow::Error> {
    let options = CopyOptions::try_from(options)?;
    let format = if let Some(format) = options.format {
        match format.to_lowercase().as_str() {
            "text" => CopyFormat::Text,
            "csv" => CopyFormat::Csv,
            "binary" => CopyFormat::Binary,
            _ => bail!("unknown FORMAT: {}", format),
        }
    } else {
        CopyFormat::Text
    };
    match (&direction, &target) {
        (CopyDirection::To, CopyTarget::Stdout) => match relation {
            CopyRelation::Table { .. } => bail!("table with COPY TO unsupported"),
            CopyRelation::Select(stmt) => {
                Ok(handle_select(scx, stmt, &Params::empty(), Some(format))?)
            }
            CopyRelation::Tail(stmt) => Ok(handle_tail(scx, stmt, Some(format))?),
        },
        _ => bail!("COPY {} {} not supported", direction, target),
    }
}

pub fn handle_insert(
    scx: &StatementContext,
    InsertStatement {
        table_name,
        columns,
        source,
    }: InsertStatement,
    params: &Params,
) -> Result<Plan, anyhow::Error> {
    let (id, mut expr) = query::plan_insert_query(scx, table_name, columns, source)?;
    expr.bind_parameters(&params)?;
    let expr = expr.decorrelate();

    Ok(Plan::Insert { id, values: expr })
}
