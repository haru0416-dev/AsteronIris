use asteroniris::core::planner::{DagContract, DagEdge, DagNode};

#[test]
fn planner_dag_valid() {
    let dag = DagContract::new(
        vec![
            DagNode::new("plan"),
            DagNode::new("build"),
            DagNode::new("verify"),
        ],
        vec![
            DagEdge::new("plan", "build"),
            DagEdge::new("build", "verify"),
        ],
    );

    dag.validate().expect("valid DAG should pass validation");
    println!("graph_valid=true");
}

#[test]
fn planner_dag_rejects_cycle() {
    let dag = DagContract::new(
        vec![DagNode::new("plan"), DagNode::new("build")],
        vec![DagEdge::new("plan", "build"), DagEdge::new("build", "plan")],
    );

    let error = dag
        .validate()
        .expect_err("cyclic DAG must be rejected")
        .to_string();

    assert_eq!(error, "cycle detected: build -> plan -> build");
}

#[test]
fn planner_dag_rejects_unknown_node_edge() {
    let dag = DagContract::new(
        vec![DagNode::new("plan")],
        vec![DagEdge::new("plan", "verify")],
    );

    let error = dag
        .validate()
        .expect_err("edge to unknown node must be rejected")
        .to_string();

    assert_eq!(
        error,
        "edge references unknown node: plan -> verify (known nodes: [plan])"
    );
}

#[test]
fn planner_dag_rejects_duplicate_edge() {
    let dag = DagContract::new(
        vec![DagNode::new("plan"), DagNode::new("verify")],
        vec![
            DagEdge::new("plan", "verify"),
            DagEdge::new("plan", "verify"),
        ],
    );

    let error = dag
        .validate()
        .expect_err("duplicate edge must be rejected")
        .to_string();

    assert_eq!(error, "duplicate edge: plan -> verify");
}
