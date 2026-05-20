use layout::backends::svg::SVGWriter;
use layout::gv::{DotParser, GraphBuilder};
use vespertide_core::TableDef;

use super::dot::render_dot;

pub fn render_svg(tables: &[TableDef]) -> Result<String, String> {
    let dot_source = render_dot(tables);
    let mut parser = DotParser::new(&dot_source);
    let graph = parser
        .process()
        .map_err(|error| format!("failed to parse ERD DOT for SVG rendering: {error}"))?;

    let mut builder = GraphBuilder::new();
    builder.visit_graph(&graph);
    let mut visual_graph = builder.get();

    let mut svg = SVGWriter::new();
    visual_graph.do_it(false, false, false, &mut svg);
    Ok(svg.finalize())
}
