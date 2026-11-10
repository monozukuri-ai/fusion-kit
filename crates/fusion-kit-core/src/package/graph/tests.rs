//! Port of the pre-implementation graph contract, using the Rust model directly.
use super::*;
use serde_json::json;

struct Fixture {
    manifest: Value,
    description: Value,
    members: BTreeSet<String>,
    tables: BTreeMap<String, Value>,
}
impl Fixture {
    fn new(kind: Value) -> Self {
        Self {
            manifest: json!({"root":"root.f3d"}),
            description: json!({"name":"Autodesk Design Description","version":"0.1","designDescription":{"designGraphs":[{
                "rootIds":[1],"designObjects":[
                    {"id":1,"relativePath":"root.f3d","contentType":"f3d","references":[{"type":kind,"ids":[2],"relationships":[{"id":2,"metadata":{"neutronRole":"role"}}]}]},
                    {"id":2,"relativePath":"part.f3d","contentType":"f3d","references":[]}
                ]
            }]}}),
            members: ["root.f3d".into(), "part.f3d".into()].into(),
            tables: BTreeMap::from([
                (
                    "root.f3d".into(),
                    json!({"name":"RedirectionsStream","schema-version":0,"designs":[{"targetFileName":"root.f3d"},{"targetFileName":"part.f3d"}],
                    "references":[{"type":kind,"from":"root.f3d","relativePath":"part.f3d","properties":[{"neutronRole":{"value":"role","dataType":"STRING"}},{"neutronData":{"value":"independent-data","dataType":"STRING"}}]}]}),
                ),
                (
                    "part.f3d".into(),
                    json!({"name":"RedirectionsStream","schema-version":0,"designs":[{"targetFileName":"part.f3d"}],"references":{}}),
                ),
            ]),
        }
    }
    fn run(&self) -> Analysis {
        analyze(
            &self.manifest,
            &self.description,
            &self.members,
            &self.tables,
        )
        .unwrap()
    }
    fn nodes(&mut self) -> &mut Vec<Value> {
        self.description["designDescription"]["designGraphs"][0]["designObjects"]
            .as_array_mut()
            .unwrap()
    }
}
fn fixture() -> Fixture {
    Fixture::new(json!("XREF"))
}
fn has(a: &Analysis, code: &str) -> bool {
    a.issues.iter().any(|d| d.code == code)
}

#[test]
fn consistent_layers_keep_independent_data() {
    let a = Fixture::new(json!("DERIVED")).run();
    assert_eq!(status(&a.issues), Status::Complete);
    assert_eq!(a.references[0].kind.as_deref(), Some("DERIVED"));
    assert_eq!(
        a.redirections
            .iter()
            .find(|r| r.document_id == "root.f3d")
            .unwrap()
            .references[0]
            .neutron_data
            .as_deref(),
        Some("independent-data")
    );
    assert!(a.comparisons.iter().all(|c| c.matches));
}
#[test]
fn missing_root_has_no_fallback() {
    let mut f = fixture();
    f.manifest = json!({});
    let a = f.run();
    assert!(has(&a, "root_missing"));
    assert!(a.root_document_id.is_none());
}
#[test]
fn absent_declared_member_is_an_error() {
    let mut f = fixture();
    f.members.remove("root.f3d");
    assert!(has(&f.run(), "root_member_missing"));
}
#[test]
fn duplicate_ids_remain_unresolved() {
    let mut f = fixture();
    let n = f.nodes()[0].clone();
    f.nodes().push(n);
    let a = f.run();
    assert!(has(&a, "duplicate_object_id"));
    assert!(a.root_document_id.is_none());
}
#[test]
fn dangling_target_and_missing_member() {
    let mut f = fixture();
    f.nodes()[0]["references"][0]["ids"] = json!([999]);
    f.members.remove("part.f3d");
    let a = f.run();
    assert!(has(&a, "dangling_reference_id"));
    assert!(has(&a, "reference_member_missing"));
}
#[test]
fn unknown_type_is_retained_at_both_layers() {
    let a = Fixture::new(json!("FUTURE")).run();
    assert_eq!(status(&a.issues), Status::Unsupported);
    assert_eq!(a.references[0].kind.as_deref(), Some("FUTURE"));
    assert_eq!(
        a.redirections
            .iter()
            .find(|r| r.document_id == "root.f3d")
            .unwrap()
            .references[0]
            .kind
            .as_deref(),
        Some("FUTURE")
    );
}
#[test]
fn malformed_kind_does_not_panic() {
    let a = Fixture::new(json!({"future":"kind"})).run();
    assert!(has(&a, "unknown_reference_type"));
    assert!(a.references[0].kind.is_none());
}
#[test]
fn content_type_agrees_with_extension() {
    let mut f = fixture();
    f.nodes()[1]["contentType"] = json!("f2d");
    assert!(has(&f.run(), "object_content_type_mismatch"));
}
#[test]
fn role_mismatch_is_reported() {
    let mut f = fixture();
    f.tables.get_mut("root.f3d").unwrap()["references"][0]["properties"][0]["neutronRole"]["value"] =
        json!("other");
    assert!(has(&f.run(), "reference_role_mismatch"));
}
#[test]
fn nonempty_leaf_object_rejected() {
    let mut f = fixture();
    f.tables.get_mut("part.f3d").unwrap()["references"] = json!({"unexpected":1});
    assert!(has(&f.run(), "redirections_references"));
}
#[test]
fn boolean_schema_is_not_zero() {
    let mut f = fixture();
    f.tables.get_mut("root.f3d").unwrap()["schema-version"] = json!(false);
    assert!(has(&f.run(), "redirections_schema"));
}
#[test]
fn boolean_relationship_id_is_not_integer() {
    let mut f = fixture();
    f.nodes()[0]["references"][0]["relationships"][0]["id"] = json!(true);
    assert!(has(&f.run(), "relationship_target"));
}
#[test]
fn duplicate_property_is_not_overwritten() {
    let mut f = fixture();
    let props = f.tables.get_mut("root.f3d").unwrap()["references"][0]["properties"]
        .as_array_mut()
        .unwrap();
    props.push(props[0].clone());
    let a = f.run();
    assert!(has(&a, "reference_property"));
    assert!(
        a.redirections
            .iter()
            .find(|r| r.document_id == "root.f3d")
            .unwrap()
            .references[0]
            .neutron_role
            .is_none()
    );
}
#[test]
fn ids_are_scoped_per_graph() {
    let mut f = fixture();
    f.description["designDescription"]["designGraphs"].as_array_mut().unwrap().push(json!({"rootIds":[1],"designObjects":[{"id":1,"relativePath":"other.f3d","contentType":"f3d","references":[]}]}));
    f.members.insert("other.f3d".into());
    assert!(!has(&f.run(), "duplicate_object_id"));
}
#[test]
fn ambiguous_root_path_unresolved() {
    let mut f = fixture();
    let mut n = f.nodes()[0].clone();
    n["id"] = json!(3);
    f.nodes().push(n);
    let a = f.run();
    assert!(has(&a, "root_object_ambiguous"));
    assert!(a.root_document_id.is_none());
}
#[test]
fn drawing_root_is_unsupported() {
    let mut f = fixture();
    f.manifest["root"] = json!("drawing.f2d");
    f.members.insert("drawing.f2d".into());
    f.nodes()[0]["relativePath"] = json!("drawing.f2d");
    f.nodes()[0]["contentType"] = json!("f2d");
    let a = f.run();
    assert_eq!(status(&a.issues), Status::Unsupported);
    assert!(a.root_document_id.is_none());
}
#[test]
fn cycles_and_unreachable_are_iterative() {
    let mut a = fixture().run();
    let mut reverse = a.references[0].clone();
    reverse.source_id = Some(2);
    reverse.target_id = Some(1);
    a.references.push(reverse);
    let (r, c) = traverse(&[1, 2, 3].into(), &a.references, &[1]).unwrap();
    assert_eq!(r, [1, 2].into());
    assert_eq!(c, vec![vec![1, 2, 1]]);
}
#[test]
fn unknown_description_schema_is_retained_without_graph_guessing() {
    let mut f = fixture();
    f.description["version"] = json!("99");
    let a = f.run();
    assert!(has(&a, "description_schema"));
    assert!(a.graphs.is_empty());
    assert!(a.root_document_id.is_none());
}

#[test]
fn relationship_fanout_and_cycle_output_have_limits() {
    let mut f = fixture();
    let group = &mut f.nodes()[0]["references"][0];
    group["ids"] = json!(vec![2; 1000]);
    group["relationships"] = json!(vec![
        json!({"id":2,"metadata":{"neutronRole":"role"}});
        1000
    ]);
    let e = analyze(&f.manifest, &f.description, &f.members, &f.tables)
        .err()
        .unwrap();
    assert_eq!(e.diagnostic.code, "limit.package_edges");
    let mut edges = Vec::new();
    let template = fixture().run().references[0].clone();
    for n in 1..600 {
        let mut edge = template.clone();
        edge.source_id = Some(n);
        edge.target_id = Some(n + 1);
        edges.push(edge);
        let mut edge = template.clone();
        edge.source_id = Some(n + 1);
        edge.target_id = Some(1);
        edges.push(edge);
    }
    assert_eq!(
        traverse(&(1..=600).collect(), &edges, &[1])
            .unwrap_err()
            .diagnostic
            .code,
        "limit.graph_cycles"
    );
}
