/*
 * Copyright 2020, Offchain Labs, Inc. All rights reserved.
 */

//! Provides types and utilities for linking together compiled mini programs

use crate::compile::{
    comma_list, CompileError, CompiledProgram, DebugInfo, ErrorSystem, FileInfo, GlobalVar,
    SourceFileMap, Type, TypeTree,
};
use crate::console::Color;
use crate::mavm::{AVMOpcode, Instruction, LabelId, Opcode, Value};
use crate::pos::{try_display_location, Location};
use crate::stringtable::StringId;
use petgraph::dot::{Config, Dot};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::DfsPostOrder;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::{DefaultHasher, HashMap};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::io::Write;
use xformcode::make_uninitialized_tuple;

use crate::compile::miniconstants::init_constant_table;
use std::path::Path;
pub use xformcode::{value_from_field_list, TupleTree, TUPLE_SIZE};

mod optimize;
mod striplabels;
mod xformcode;

#[derive(Clone, Serialize, Deserialize)]
pub struct SerializableTypeTree {
    inner: BTreeMap<String, (Type, String)>,
}

impl SerializableTypeTree {
    pub fn from_type_tree(tree: TypeTree) -> Self {
        let mut inner = BTreeMap::new();
        for ((path, id), tipe) in tree.into_iter() {
            inner.insert(format!("{}, {}", comma_list(&path), id.id), tipe);
        }
        Self { inner }
    }
    pub fn into_type_tree(self, fix: bool) -> TypeTree {
        let mut type_tree = HashMap::new();
        for (path, tipe) in self.inner.into_iter() {
            let mut x: Vec<_> = path.split(", ").map(|val| val.to_string()).collect();
            let id = x.pop().expect("empty list");
            let sid = StringId::new(if fix { x.clone() } else { vec![] }, id.clone());
            type_tree.insert((x, sid), tipe);
        }
        type_tree
    }
}

/// Represents a mini program that has gone through the post-link compilation step.
///
/// This is typically constructed via the `postlink_compile` function.
#[derive(Serialize, Deserialize)]
pub struct LinkedProgram {
    #[serde(default)]
    pub arbos_version: u64,
    pub code: Vec<Instruction<AVMOpcode>>,
    pub static_val: Value,
    pub globals: Vec<GlobalVar>,
    // #[serde(default)]
    pub file_info_chart: BTreeMap<u64, FileInfo>,
    pub type_tree: SerializableTypeTree,
}

impl LinkedProgram {
    /// Serializes self to the format specified by the format argument, with a default of json for
    /// None. The output is written to a dynamically dispatched implementor of `std::io::Write`,
    /// specified by the output argument.
    pub fn to_output(&self, output: &mut dyn io::Write, format: Option<&str>) {
        match format {
            Some("pretty") => {
                writeln!(output, "static: {}", self.static_val).unwrap();
                for (idx, insn) in self.code.iter().enumerate() {
                    writeln!(
                        output,
                        "{:05}:  {} \t\t {}",
                        idx,
                        insn,
                        try_display_location(
                            insn.debug_info.location,
                            &self.file_info_chart,
                            false
                        )
                    )
                    .unwrap();
                }
            }
            None | Some("json") => match serde_json::to_string(self) {
                Ok(prog_str) => {
                    writeln!(output, "{}", prog_str).unwrap();
                }
                Err(e) => {
                    eprintln!("failure");
                    writeln!(output, "json serialization error: {:?}", e).unwrap();
                }
            },
            Some("bincode") => match bincode::serialize(self) {
                Ok(encoded) => {
                    if let Err(e) = output.write_all(&encoded) {
                        writeln!(output, "bincode write error: {:?}", e).unwrap();
                    }
                }
                Err(e) => {
                    writeln!(output, "bincode serialization error: {:?}", e).unwrap();
                }
            },
            Some(weird_value) => {
                writeln!(output, "invalid format: {}", weird_value).unwrap();
            }
        }
    }
}

/// Represents an import generated by a `use` statement.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Import {
    /// Module path, relative to logical program root.
    pub path: Vec<String>,
    /// Name of `Type` or function to be imported.
    pub name: String,
    /// Unique global id this import refers to
    pub unique_id: LabelId,
    /// `StringId` of the use-statement from parsing according to the containing module's `StringTable`
    pub id: Option<StringId>,
    /// Location of the use-statement in code
    pub location: Option<Location>,
}

impl Import {
    pub fn new(
        path: Vec<String>,
        name: String,
        id: Option<StringId>,
        location: Option<Location>,
    ) -> Self {
        let unique_id = Import::unique_id(&path, &name);
        Import {
            path,
            name,
            unique_id,
            id,
            location,
        }
    }

    pub fn loc(&self) -> Vec<Location> {
        self.location.into_iter().collect()
    }

    pub fn new_builtin(virtual_file: &str, name: &str) -> Self {
        let path = vec!["core".to_string(), virtual_file.to_string()];
        let name = name.to_string();
        let unique_id = Import::unique_id(&path, &name);
        Import {
            path,
            name,
            unique_id,
            id: None,
            location: None,
        }
    }

    pub fn unique_id(path: &Vec<String>, name: &String) -> LabelId {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        name.hash(&mut hasher);
        hasher.finish()
    }
}

fn hardcode_jump_table_into_register(
    code: &mut Vec<Instruction>,
    jump_table: &Value,
    test_mode: bool,
) {
    let offset = if test_mode { 1 } else { 2 };
    let old_imm = code[offset].clone().immediate.unwrap();
    code[offset] = Instruction::from_opcode_imm(
        code[offset].opcode,
        old_imm.replace_last_none(jump_table),
        code[offset].debug_info,
    );
}

pub type ProgGraph = DiGraph<CompiledProgram, usize>;

/// Creates a graph of the `CompiledProgram`s and then combines them into a single
/// `CompiledProgram` in such a way as to reduce the number of backward jumps.
pub fn link(
    progs: Vec<CompiledProgram>,
    globals: Vec<GlobalVar>,
    error_system: &mut ErrorSystem,
    test_mode: bool,
) -> CompiledProgram {
    let mut merged_source_file_map = SourceFileMap::new_empty();
    let mut merged_file_info_chart = HashMap::new();
    let type_tree = progs[0].type_tree.clone();

    let mut graph = ProgGraph::new();
    let mut id_to_node = HashMap::new();

    for prog in progs {
        merged_source_file_map.push(
            prog.code.len(),
            match &prog.source_file_map {
                Some(sfm) => sfm.get(0),
                None => "".to_string(),
            },
        );
        merged_file_info_chart.extend(prog.file_info_chart.clone());
        let func_id = prog.unique_id;
        let node = graph.add_node(prog);
        id_to_node.insert(func_id, node);
    }

    for node in graph.node_indices() {
        let prog = &graph[node];

        let uniques = prog.code.iter().flat_map(|insn| insn.get_uniques());

        let mut usages = BTreeMap::new();
        for unique in uniques {
            *usages.entry(unique).or_insert(0) += 1;
        }

        for (unique, count) in usages {
            let dest = *id_to_node.get(&unique).unwrap();
            if node != dest {
                graph.add_edge(node, dest, count);
            }
        }
    }

    let mut debug_info = DebugInfo::default();
    debug_info.attributes.codegen_print = globals
        .iter()
        .any(|x| x.debug_info.attributes.codegen_print);

    // Initialize globals or allow jump table retrieval
    let mut linked_code = if test_mode {
        vec![
            Instruction::from_opcode_imm(
                Opcode::AVMOpcode(AVMOpcode::Noop),
                Value::none(),
                debug_info,
            ),
            Instruction::from_opcode_imm(
                Opcode::AVMOpcode(AVMOpcode::Rset),
                make_uninitialized_tuple(globals.len()),
                debug_info,
            ),
        ]
    } else {
        vec![
            Instruction::from_opcode(Opcode::AVMOpcode(AVMOpcode::Rpush), debug_info),
            Instruction::from_opcode_imm(
                Opcode::AVMOpcode(AVMOpcode::Noop),
                Value::none(),
                debug_info,
            ),
            Instruction::from_opcode_imm(
                Opcode::AVMOpcode(AVMOpcode::Rset),
                make_uninitialized_tuple(globals.len()),
                debug_info,
            ),
        ]
    };

    let main = NodeIndex::from(0);
    let mut dfs = DfsPostOrder::new(&graph, main);
    let mut traversal = vec![];
    while let Some(node) = dfs.next(&graph) {
        traversal.push(node);
    }
    traversal.reverse();

    let mut unvisited: HashSet<_> = graph.node_indices().collect();
    for node in traversal {
        unvisited.remove(&node);
        let prog = &graph[node];
        linked_code.append(&mut prog.code.clone());
    }

    for node in graph.node_indices() {
        let name = &graph[node].name;
        let path = &graph[node].path;
        let debug_info = &graph[node].debug_info;

        if ["core", "std", "std2", "meta"].contains(&path[0].as_str()) {
            continue;
        }

        if unvisited.contains(&node) && !name.starts_with('_') {
            error_system.warnings.push(CompileError::new_warning(
                String::from("Compile warning"),
                format!(
                    "func {} is unreachable",
                    Color::color(error_system.warn_color, name)
                ),
                debug_info.locs(),
            ));
        }
    }

    let graph = graph.map(|_, prog| prog.name.clone(), |_, e| e);

    let mut file = File::create("callgraph.dot").expect("failed to open file");
    let dot = Dot::with_config(&graph, &[Config::EdgeNoLabel]);
    writeln!(&mut file, "{:?}", dot).expect("failed to write .dot file");

    // check for unvisited

    CompiledProgram::new(
        String::from("entry_point"),
        vec![String::from("meta"), String::from("link")],
        linked_code,
        globals,
        Some(merged_source_file_map),
        merged_file_info_chart,
        type_tree,
        DebugInfo::default(),
    )
}

/// Converts a linked `CompiledProgram` into a `LinkedProgram` by fixing non-forward jumps,
/// converting wide tuples to nested tuples, performing code optimizations, converting the jump
/// table to a static value, and combining the file info chart with the associated argument.
pub fn postlink_compile(
    program: CompiledProgram,
    mut file_info_chart: BTreeMap<u64, FileInfo>,
    _error_system: &mut ErrorSystem,
    test_mode: bool,
    debug: bool,
) -> Result<LinkedProgram, CompileError> {
    let consider_debug_printing = |code: &Vec<Instruction>, did_print: bool, phase: &str| {
        if debug {
            println!("========== {} ==========", phase);
            for (idx, insn) in code.iter().enumerate() {
                println!(
                    "{}  {}",
                    Color::grey(format!("{:04}", idx)),
                    insn.pretty_print(Color::PINK)
                );
            }
        } else if did_print {
            println!("========== {} ==========", phase);
            for (idx, insn) in code.iter().enumerate() {
                if insn.debug_info.attributes.codegen_print {
                    println!(
                        "{}  {}",
                        Color::grey(format!("{:04}", idx)),
                        insn.pretty_print(Color::PINK)
                    );
                }
            }
        }
    };

    let mut did_print = false;

    if debug {
        println!("========== after initial linking ===========");
        for (idx, insn) in program.code.iter().enumerate() {
            println!(
                "{}  {}",
                Color::grey(format!("{:04}", idx)),
                insn.pretty_print(Color::PINK)
            );
        }
    } else {
        for (idx, insn) in program.code.iter().enumerate() {
            if insn.debug_info.attributes.codegen_print {
                println!(
                    "{}  {}",
                    Color::grey(format!("{:04}", idx)),
                    insn.pretty_print(Color::PINK)
                );
                did_print = true;
            }
        }
    }
    let (code_2, jump_table) =
        striplabels::fix_nonforward_labels(&program.code, program.globals.len() - 1);
    consider_debug_printing(&code_2, did_print, "after fix_backward_labels");

    let code_3 = xformcode::fix_tuple_size(&code_2, program.globals.len())?;
    consider_debug_printing(&code_3, did_print, "after fix_tuple_size");

    let code_4 = optimize::peephole(&code_3);
    consider_debug_printing(&code_4, did_print, "after peephole optimization");

    let (mut code_5, jump_table_final) = striplabels::strip_labels(code_4, &jump_table)?;
    let jump_table_len = jump_table_final.len();
    let jump_table_value = xformcode::jump_table_to_value(jump_table_final);

    hardcode_jump_table_into_register(&mut code_5, &jump_table_value, test_mode);
    let code_final: Vec<_> = code_5
        .into_iter()
        .map(|insn| {
            if let Opcode::AVMOpcode(inner) = insn.opcode {
                Ok(Instruction::new(inner, insn.immediate, insn.debug_info))
            } else {
                Err(CompileError::new(
                    String::from("Postlink error"),
                    format!("In final output encountered virtual opcode {}", insn.opcode),
                    insn.debug_info.location.into_iter().collect(),
                ))
            }
        })
        .collect::<Result<Vec<_>, CompileError>>()?;

    if debug {
        println!("============ after strip_labels =============");
        println!("static: {}", jump_table_value);
        for (idx, insn) in code_final.iter().enumerate() {
            println!("{:04}  {}", idx, insn);
        }
        println!("============ after full compile/link =============");
    }

    if debug {
        let globals_shape = make_uninitialized_tuple(program.globals.len());
        println!(
            "\nGlobal Vars {}\n{}\n",
            program.globals.len(),
            globals_shape.pretty_print(Color::PINK)
        );
        let jump_shape = make_uninitialized_tuple(jump_table_len);
        println!(
            "Jump Table {}\n{}\n",
            jump_table_len,
            jump_shape.pretty_print(Color::PINK)
        );

        let size = code_final.iter().count() as f64;
        println!("Total Instructions {}", size);
    }

    file_info_chart.extend(program.file_info_chart.clone());

    Ok(LinkedProgram {
        arbos_version: init_constant_table(Some(Path::new("arb_os/constants.json")))
            .unwrap()
            .get("ArbosVersionNumber")
            .unwrap()
            .clone()
            .trim_to_u64(),
        code: code_final,
        static_val: Value::none(),
        globals: program.globals.clone(),
        file_info_chart,
        type_tree: SerializableTypeTree::from_type_tree(program.type_tree),
    })
}
