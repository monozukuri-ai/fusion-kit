use super::{GraphInfo, GraphNode, LayerComparison, RedirectionInfo, ReferenceInfo, path_valid};
use crate::{Diagnostic, Error, ErrorKind, Status};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

pub(super) struct Analysis {
    pub declared_root: Option<String>,
    pub root_document_id: Option<String>,
    pub graphs: Vec<GraphInfo>,
    pub references: Vec<ReferenceInfo>,
    pub redirections: Vec<RedirectionInfo>,
    pub comparisons: Vec<LayerComparison>,
    pub undocumented_members: Vec<String>,
    pub issues: Vec<Diagnostic>,
}

pub(super) fn status(issues: &[Diagnostic]) -> Status {
    if issues.iter().any(|d| d.severity == "error") {
        Status::Invalid
    } else if issues.iter().any(|d| d.severity == "unsupported") {
        Status::Unsupported
    } else if !issues.is_empty() {
        Status::Partial
    } else {
        Status::Complete
    }
}

struct Check {
    issues: Vec<Diagnostic>,
    records: usize,
    output_bytes: usize,
}
impl Check {
    fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.output_bytes = self.output_bytes.saturating_add(bytes);
        if self.output_bytes > 64 * 1024 * 1024 {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                "limit.package_output",
                "relationships",
                "normalized graph text exceeds 64 MiB",
            ));
        }
        Ok(())
    }
    fn note(
        &mut self,
        code: &'static str,
        member: &str,
        message: impl Into<String>,
        severity: &'static str,
    ) {
        self.issues.push(Diagnostic {
            severity,
            code,
            stage: "relationships",
            member: Some(member.into()),
            source: None,
            offset: None,
            offset_basis: None,
            context: None,
            message: message.into(),
        });
    }
    fn array<'a>(&mut self, value: Option<&'a Value>, member: &str) -> Result<&'a [Value], Error> {
        match value.and_then(Value::as_array) {
            Some(a) => {
                self.records += a.len();
                if self.records > 100_000 {
                    return Err(Error::new(
                        ErrorKind::ResourceLimit,
                        "limit.package_records",
                        "relationships",
                        "package exceeds 100000 records",
                    )
                    .member(member));
                }
                Ok(a)
            }
            None => {
                self.note("expected_array", member, "expected JSON array", "error");
                Ok(&[])
            }
        }
    }
    fn kind(&mut self, value: Option<&Value>, member: &str) -> Option<String> {
        let kind = value.and_then(Value::as_str).map(str::to_owned);
        if !matches!(kind.as_deref(), Some("XREF" | "DERIVED")) {
            self.note(
                "unknown_reference_type",
                member,
                "reference type is neither XREF nor DERIVED; original JSON retained",
                "unsupported",
            );
        }
        kind
    }
    fn path(&mut self, value: Option<&Value>, member: &str, code: &'static str) -> Option<String> {
        if let Some(path) = value.and_then(Value::as_str).filter(|p| path_valid(p)) {
            Some(path.into())
        } else {
            self.note(
                code,
                member,
                "expected a safe, nonempty member path",
                "error",
            );
            None
        }
    }
}

pub(super) fn analyze(
    manifest: &Value,
    description: &Value,
    members: &BTreeSet<String>,
    tables: &BTreeMap<String, Value>,
) -> Result<Analysis, Error> {
    let mut c = Check {
        issues: Vec::new(),
        records: 0,
        output_bytes: 0,
    };
    let root = c.path(manifest.get("root"), "Manifest.json", "root_missing");
    let mut root_eligible = false;
    if let Some(root) = &root {
        if !members.contains(root) {
            c.note("root_member_missing", "Manifest.json", root, "error");
        } else if !root.to_ascii_lowercase().ends_with(".f3d") {
            c.note(
                if root.to_ascii_lowercase().ends_with(".f2d") {
                    "drawing_root"
                } else {
                    "unsupported_root"
                },
                "Manifest.json",
                "root requires another document decoder",
                "unsupported",
            );
        } else {
            root_eligible = true;
        }
    }
    let mut redirections = Vec::new();
    for (path, table) in tables {
        let context = format!("{path}!RedirectionsStream.dat");
        if !table.is_object() {
            c.note(
                "redirections_shape",
                &context,
                "expected an object",
                "error",
            );
            continue;
        }
        if table.get("name").and_then(Value::as_str) != Some("RedirectionsStream") {
            c.note(
                "redirections_name",
                &context,
                "unexpected table name",
                "error",
            );
        }
        if table.get("schema-version").and_then(Value::as_i64) != Some(0) {
            c.note(
                "redirections_schema",
                &context,
                "unsupported schema; payload retained without interpretation",
                "unsupported",
            );
            continue;
        }
        let designs = c.array(table.get("designs"), &context)?;
        if designs
            .first()
            .and_then(|d| d.get("targetFileName"))
            .and_then(Value::as_str)
            != Some(path)
        {
            c.note(
                "redirections_self",
                &context,
                "first design must identify its containing F3D member",
                "error",
            );
        }
        let mut targets = BTreeSet::new();
        for design in designs {
            if let Some(target) = c.path(design.get("targetFileName"), &context, "design_path")
                && !targets.insert(target)
            {
                c.note(
                    "duplicate_design_path",
                    &context,
                    "duplicate design targetFileName",
                    "error",
                );
            }
        }
        let empty = Value::Array(Vec::new());
        let refs = match table.get("references") {
            Some(Value::Object(o)) if o.is_empty() => Some(&empty),
            value => value,
        };
        if refs.is_none_or(|v| !v.is_array()) {
            c.note(
                "redirections_references",
                &context,
                "references must be array or empty object",
                "error",
            );
        }
        let mut normalized = Vec::new();
        for (index, r) in c.array(refs, &context)?.iter().enumerate() {
            if !r.is_object() {
                c.note(
                    "reference_shape",
                    &context,
                    "expected reference object",
                    "error",
                );
                continue;
            }
            let mut values: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
            for prop in c.array(r.get("properties"), &context)? {
                if let Some(obj) = prop.as_object() {
                    for (k, v) in obj {
                        values.entry(k).or_default().push(v);
                    }
                }
            }
            let mut prop_value = |key: &'static str| {
                let candidates = values.get(key);
                let valid = candidates.filter(|a| a.len() == 1).and_then(|a| a.first());
                let text = valid
                    .filter(|v| v.get("dataType").and_then(Value::as_str) == Some("STRING"))
                    .and_then(|v| v.get("value"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if text.is_none() {
                    c.note(
                        "reference_property",
                        &context,
                        format!("exactly one STRING {key} is required"),
                        "error",
                    );
                }
                text
            };
            let role = prop_value("neutronRole");
            let data = prop_value("neutronData");
            if role.as_deref() == Some("") {
                c.note(
                    "reference_role_empty",
                    &context,
                    "neutronRole must be nonempty",
                    "error",
                );
            }
            let source = c.path(r.get("from"), &context, "reference_source_mismatch");
            if source.as_deref() != Some(path) {
                c.note(
                    "reference_source_mismatch",
                    &context,
                    "from differs from containing member",
                    "error",
                );
            }
            let target = c.path(r.get("relativePath"), &context, "reference_member_missing");
            if !target.as_ref().is_some_and(|t| members.contains(t)) {
                c.note(
                    "reference_member_missing",
                    &context,
                    "target member is absent",
                    "error",
                );
            }
            if !target.as_ref().is_some_and(|t| targets.contains(t)) {
                c.note(
                    "reference_design_missing",
                    &context,
                    "target is absent from designs table",
                    "error",
                );
            }
            let kind = c.kind(r.get("type"), &context);
            c.charge(
                source.as_ref().map_or(0, String::len)
                    + target.as_ref().map_or(0, String::len)
                    + kind.as_ref().map_or(0, String::len)
                    + role.as_ref().map_or(0, String::len)
                    + data.as_ref().map_or(0, String::len),
            )?;
            normalized.push(ReferenceInfo {
                layer: "redirections",
                graph_index: None,
                source_id: None,
                target_id: None,
                source,
                target,
                kind,
                neutron_role: role,
                neutron_data: data,
                group_index: index,
                relationship_index: None,
            });
        }
        redirections.push(RedirectionInfo {
            document_id: path.clone(),
            designs: designs.to_vec(),
            references: normalized,
        });
    }
    let mut graphs = Vec::new();
    let mut expanded_edges = 0;
    let mut root_objects = Vec::new();
    let mut described = BTreeSet::new();
    // A missing schema marker is not treated as version 0.1.
    let supported = description.get("name").and_then(Value::as_str)
        == Some("Autodesk Design Description")
        && description.get("version").and_then(Value::as_str) == Some("0.1");
    if !supported {
        c.note(
            "description_schema",
            "DesignDescription.json",
            "unknown design description schema; payload retained",
            "unsupported",
        );
    }
    let empty = Value::Array(Vec::new());
    let raw_graphs = if supported {
        description
            .get("designDescription")
            .and_then(|v| v.get("designGraphs"))
    } else {
        Some(&empty)
    };
    for (gi, g) in c
        .array(raw_graphs, "DesignDescription.json")?
        .iter()
        .enumerate()
    {
        let ctx = format!("DesignDescription.json.graph[{gi}]");
        if !g.is_object() {
            c.note("graph_shape", &ctx, "expected object", "error");
            continue;
        }
        let mut ids: BTreeMap<i64, Vec<&Value>> = BTreeMap::new();
        for obj in c.array(g.get("designObjects"), &ctx)? {
            if let Some(id) = obj.get("id").and_then(Value::as_i64) {
                ids.entry(id).or_default().push(obj);
            } else {
                c.note(
                    "object_id",
                    &ctx,
                    "object requires signed 64-bit integer ID",
                    "error",
                );
            }
        }
        for (id, rows) in &ids {
            if rows.len() != 1 {
                c.note("duplicate_object_id", &ctx, id.to_string(), "error");
            }
        }
        let unique: BTreeMap<i64, &Value> = ids
            .into_iter()
            .filter(|(_, v)| v.len() == 1)
            .map(|(k, v)| (k, v[0]))
            .collect();
        let mut roots = Vec::new();
        for value in c.array(g.get("rootIds"), &ctx)? {
            if let Some(id) = value.as_i64().filter(|id| unique.contains_key(id)) {
                roots.push(id);
            } else {
                c.note("root_id_unresolved", &ctx, format!("{value:?}"), "error");
            }
        }
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        for (&id, obj) in &unique {
            let path = c.path(obj.get("relativePath"), &ctx, "object_path");
            let content_type = obj
                .get("contentType")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Some(path) = &path {
                described.insert(path.clone());
                if !members.contains(path) {
                    c.note("object_member_missing", &ctx, path, "error");
                }
                let suffix = path.rsplit('.').next().unwrap_or("");
                if !content_type
                    .as_ref()
                    .is_some_and(|t| t.eq_ignore_ascii_case(suffix))
                {
                    c.note("object_content_type_mismatch", &ctx, path, "error");
                }
                if Some(path) == root.as_ref() {
                    root_objects.push((gi, id, roots.contains(&id)));
                    if !roots.contains(&id) {
                        c.note(
                            "root_declaration_mismatch",
                            &ctx,
                            "Manifest root absent from rootIds",
                            "error",
                        );
                    }
                }
            }
            nodes.push(GraphNode {
                object_id: id,
                path: path.clone(),
                content_type,
            });
            for (group_index, group) in c.array(obj.get("references"), &ctx)?.iter().enumerate() {
                if !group.is_object() {
                    c.note("reference_shape", &ctx, "expected reference group", "error");
                    continue;
                }
                let kind = c.kind(group.get("type"), &ctx);
                let targets = c.array(group.get("ids"), &ctx)?;
                let rels = c.array(group.get("relationships").or(Some(&empty)), &ctx)?;
                let target_ids: BTreeSet<i64> = targets.iter().filter_map(Value::as_i64).collect();
                let mut by_target: BTreeMap<i64, Vec<Relation<'_>>> = BTreeMap::new();
                for (index, rel) in rels.iter().enumerate() {
                    if !rel
                        .get("id")
                        .and_then(Value::as_i64)
                        .is_some_and(|n| target_ids.contains(&n))
                    {
                        c.note(
                            "relationship_target",
                            &ctx,
                            "relationship ID absent from ids",
                            "error",
                        );
                    }
                    if let Some(id) = rel.get("id").and_then(Value::as_i64) {
                        by_target
                            .entry(id)
                            .or_default()
                            .push((Some(index), Some(rel)));
                    }
                }
                for target in targets {
                    let target_id = target.as_i64();
                    let target_obj = target_id.and_then(|id| unique.get(&id));
                    if target_obj.is_none() {
                        c.note(
                            "dangling_reference_id",
                            &ctx,
                            format!("{target:?}"),
                            "error",
                        );
                    }
                    let selected = target_id
                        .and_then(|id| by_target.get(&id))
                        .map(Vec::as_slice)
                        .unwrap_or(&[(None, None)]);
                    for &(ri, relation) in selected {
                        expanded_edges += 1;
                        if expanded_edges > 100_000 {
                            return Err(Error::new(
                                ErrorKind::ResourceLimit,
                                "limit.package_edges",
                                "relationships",
                                "too many expanded reference relationships",
                            )
                            .member(&ctx));
                        }
                        let role = relation
                            .and_then(|r| r.get("metadata"))
                            .and_then(|m| m.get("neutronRole"))
                            .and_then(Value::as_str)
                            .filter(|v| !v.is_empty())
                            .map(str::to_owned);
                        c.charge(
                            path.as_ref().map_or(0, String::len)
                                + role.as_ref().map_or(0, String::len)
                                + kind.as_ref().map_or(0, String::len)
                                + target_obj
                                    .and_then(|v| v.get("relativePath"))
                                    .and_then(Value::as_str)
                                    .map_or(0, str::len),
                        )?;
                        if role.is_none() {
                            c.note(
                                "relationship_role_missing",
                                &ctx,
                                format!("{target:?}"),
                                "warning",
                            );
                        }
                        edges.push(ReferenceInfo {
                            layer: "graph",
                            graph_index: Some(gi),
                            source_id: Some(id),
                            target_id,
                            source: path.clone(),
                            target: target_obj
                                .and_then(|v| v.get("relativePath"))
                                .and_then(Value::as_str)
                                .filter(|p| path_valid(p))
                                .map(str::to_owned),
                            kind: kind.clone(),
                            neutron_role: role,
                            neutron_data: None,
                            group_index,
                            relationship_index: ri,
                        });
                        if edges.len() > 100_000 {
                            return Err(Error::new(
                                ErrorKind::ResourceLimit,
                                "limit.package_edges",
                                "relationships",
                                "too many expanded reference relationships",
                            )
                            .member(&ctx));
                        }
                    }
                }
            }
        }
        let (reachable, cycles) = traverse(&unique.keys().copied().collect(), &edges, &roots)?;
        for cycle in &cycles {
            c.note("reference_cycle", &ctx, format!("{cycle:?}"), "error");
        }
        let unreachable: Vec<i64> = unique
            .keys()
            .filter(|id| !reachable.contains(id))
            .copied()
            .collect();
        if !unreachable.is_empty() {
            c.note(
                "unreachable_objects",
                &ctx,
                format!("{unreachable:?}"),
                "warning",
            );
        }
        graphs.push(GraphInfo {
            graph_index: gi,
            root_ids: roots,
            nodes,
            references: edges,
            reachable_ids: reachable.into_iter().collect(),
            unreachable_ids: unreachable,
            cycles,
        });
    }
    if supported {
        if root.is_some() && root_objects.is_empty() {
            c.note(
                "root_object_missing",
                "DesignDescription.json",
                "root has no unique graph object",
                "error",
            );
        }
        if root_objects.len() > 1 {
            c.note(
                "root_object_ambiguous",
                "DesignDescription.json",
                "multiple graph objects identify root",
                "error",
            );
        }
    }
    let root_document_id = if root_eligible && root_objects.len() == 1 && root_objects[0].2 {
        root.clone()
    } else {
        None
    };
    let mut comparisons = Vec::new();
    let inner_by_path: BTreeMap<_, _> = redirections.iter().map(|r| (&r.document_id, r)).collect();
    for graph in &graphs {
        let mut outgoing: BTreeMap<i64, Vec<&ReferenceInfo>> = BTreeMap::new();
        for r in &graph.references {
            if let Some(id) = r.source_id {
                outgoing.entry(id).or_default().push(r);
            }
        }
        for node in &graph.nodes {
            let Some(path) = &node.path else {
                continue;
            };
            let Some(inner) = inner_by_path.get(path) else {
                if path.to_ascii_lowercase().ends_with(".f3d") {
                    c.note(
                        "redirections_unavailable",
                        path,
                        "inner table unavailable or unsupported",
                        "warning",
                    );
                }
                continue;
            };
            let outer = outgoing.get(&node.object_id).cloned().unwrap_or_default();
            let matches = multiset(outer.iter().copied()) == multiset(inner.references.iter());
            comparisons.push(LayerComparison {
                graph_index: graph.graph_index,
                source: path.clone(),
                matches,
            });
            if !matches {
                c.note(
                    "reference_layers_differ",
                    path,
                    "outer graph and inner reference multiset differ",
                    "error",
                );
                let roles = |rows: Vec<&ReferenceInfo>| {
                    let mut out: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
                    for r in rows {
                        out.entry((r.target.clone(), r.kind.clone()))
                            .or_default()
                            .insert(r.neutron_role.clone());
                    }
                    out
                };
                let a = roles(outer);
                let b = roles(inner.references.iter().collect());
                if a.iter()
                    .any(|(k, v)| b.get(k).is_some_and(|other| other != v))
                {
                    c.note(
                        "reference_role_mismatch",
                        path,
                        "roles differ for same target and type",
                        "error",
                    );
                }
            }
        }
    }
    let undocumented: Vec<String> = members
        .iter()
        .filter(|p| p.to_ascii_lowercase().ends_with(".f3d") && !described.contains(*p))
        .cloned()
        .collect();
    if !undocumented.is_empty() {
        c.note(
            "undocumented_members",
            "archive",
            format!("{undocumented:?}"),
            "warning",
        );
    }
    let references = graphs.iter().flat_map(|g| g.references.clone()).collect();
    Ok(Analysis {
        declared_root: root,
        root_document_id,
        graphs,
        references,
        redirections,
        comparisons,
        undocumented_members: undocumented,
        issues: c.issues,
    })
}

type ReferenceKey = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);
type Relation<'a> = (Option<usize>, Option<&'a Value>);
fn multiset<'a>(rows: impl Iterator<Item = &'a ReferenceInfo>) -> BTreeMap<ReferenceKey, usize> {
    let mut out = BTreeMap::new();
    for r in rows {
        *out.entry((
            r.source.clone(),
            r.target.clone(),
            r.kind.clone(),
            r.neutron_role.clone(),
        ))
        .or_default() += 1;
    }
    out
}

fn traverse(
    nodes: &BTreeSet<i64>,
    edges: &[ReferenceInfo],
    roots: &[i64],
) -> Result<(BTreeSet<i64>, Vec<Vec<i64>>), Error> {
    let mut adj: BTreeMap<i64, Vec<i64>> = nodes.iter().map(|n| (*n, Vec::new())).collect();
    for edge in edges {
        if let (Some(from), Some(to)) = (edge.source_id, edge.target_id)
            && nodes.contains(&to)
            && let Some(rows) = adj.get_mut(&from)
        {
            rows.push(to);
        }
    }
    let mut reachable = BTreeSet::new();
    let mut pending = roots.to_vec();
    while let Some(n) = pending.pop() {
        if let Some(rows) = adj.get(&n)
            && reachable.insert(n)
        {
            pending.extend(rows);
        }
    }
    let mut colors = BTreeMap::new();
    let mut cycles = Vec::new();
    let mut cycle_items = 0;
    for start in nodes {
        if colors.contains_key(start) {
            continue;
        }
        let mut stack = vec![(*start, 0)];
        let mut path = vec![*start];
        let mut positions = BTreeMap::from([(*start, 0)]);
        colors.insert(*start, 1);
        while let Some((n, index)) = stack.last_mut() {
            if let Some(&child) = adj[n].get(*index) {
                *index += 1;
                match colors.get(&child) {
                    Some(1) => {
                        cycle_items += path.len() - positions[&child] + 1;
                        if cycle_items > 100_000 {
                            return Err(Error::new(
                                ErrorKind::ResourceLimit,
                                "limit.graph_cycles",
                                "relationships",
                                "cycle output exceeds 100000 IDs",
                            ));
                        }
                        let mut cycle = path[positions[&child]..].to_vec();
                        cycle.push(child);
                        cycles.push(cycle);
                    }
                    Some(_) => {}
                    None => {
                        colors.insert(child, 1);
                        positions.insert(child, path.len());
                        path.push(child);
                        stack.push((child, 0));
                    }
                }
            } else {
                let n = *n;
                stack.pop();
                positions.remove(&n);
                path.pop();
                colors.insert(n, 2);
            }
        }
    }
    Ok((reachable, cycles))
}
