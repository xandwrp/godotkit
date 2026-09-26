use crate::render::Human;
use gdview::net::{self, RpcCall, RpcEndpoint, Spawner, Synchronizer, Unknown};
use std::io::Write;

#[derive(serde::Serialize)]
#[serde(transparent)]
pub(super) struct Report(pub net::NetReport);
#[derive(serde::Serialize)]
#[serde(transparent)]
pub(super) struct Explanation(pub net::Explanation);

impl Human for Report {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let r = &self.0;
        writeln!(
            out,
            "Multiplayer source map (offline; schema {})",
            r.schema_version
        )?;
        writeln!(
            out,
            "{} scripts, {} scenes, {} text resources",
            r.coverage.scripts_scanned, r.coverage.scenes_scanned, r.coverage.resources_scanned
        )?;
        observations(out, &r.endpoints, &r.calls, &r.spawners, &r.synchronizers)?;
        writeln!(
            out,
            "\nAuthority observations ({}):",
            r.authority_uses.len()
        )?;
        for a in &r.authority_uses {
            writeln!(out, "  {}:{}  {}", a.location.path, a.location.line, a.call)?;
        }
        writeln!(out, "\nAutoloads (source evidence):")?;
        for a in &r.autoloads {
            let target = a
                .autoload
                .path
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unresolved".into());
            writeln!(
                out,
                "  {}  {target}  networked={}",
                a.autoload.name, a.networked
            )?;
        }
        writeln!(
            out,
            "\nObserved multiplayer contexts (not necessarily active):"
        )?;
        for c in &r.contexts {
            writeln!(
                out,
                "  {}:{}  root={:?} api={}",
                c.location.path,
                c.location.line,
                c.root.as_ref().map(|p| p.0.as_str()),
                c.api.as_deref().unwrap_or("unknown")
            )?;
        }
        unknowns(out, &r.unknowns)?;
        for limit in &r.coverage.limitations {
            writeln!(out, "note: {limit}")?;
        }
        Ok(())
    }
}

impl Human for Explanation {
    fn human(&self, out: &mut dyn Write) -> std::io::Result<()> {
        let e = &self.0;
        writeln!(out, "Multiplayer explanation: {}", e.query)?;
        observations(out, &e.endpoints, &e.calls, &e.spawners, &e.synchronizers)?;
        for anchor in &e.anchors {
            writeln!(
                out,
                "  anchor {}  {}:{}  runtime={:?} context_candidates={:?}",
                anchor.script,
                anchor
                    .scene
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "autoload".into()),
                anchor.node.0,
                anchor.runtime_path.as_ref().map(|p| p.0.as_str()),
                anchor.context_candidates
            )?;
        }
        for (index, context) in e.contexts.iter().enumerate() {
            writeln!(
                out,
                "  context [{index}] {}:{}  observed root={:?} api={}",
                context.location.path,
                context.location.line,
                context.root.as_ref().map(|p| p.0.as_str()),
                context.api.as_deref().unwrap_or("unknown")
            )?;
        }
        for a in &e.authority_uses {
            writeln!(
                out,
                "  authority {}:{}  {}",
                a.location.path, a.location.line, a.call
            )?;
        }
        unknowns(out, &e.unknowns)?;
        for note in &e.notes {
            writeln!(out, "note: {note}")?;
        }
        Ok(())
    }
}

fn observations(
    out: &mut dyn Write,
    endpoints: &[RpcEndpoint],
    calls: &[RpcCall],
    spawners: &[Spawner],
    synchronizers: &[Synchronizer],
) -> std::io::Result<()> {
    writeln!(out, "\nRPC endpoints ({}):", endpoints.len())?;
    for (i, e) in endpoints.iter().enumerate() {
        let class = e
            .class
            .as_ref()
            .map(|c| format!("{c}."))
            .unwrap_or_default();
        writeln!(
            out,
            "  [{i}] {}:{}  {class}{}  {:?} {:?} channel={} call_local={}",
            e.script,
            e.location.line,
            e.method,
            e.config.mode,
            e.config.transfer,
            e.config.channel,
            e.config.call_local
        )?;
    }
    writeln!(
        out,
        "\nRPC call observations ({}; candidates are not compatibility proof):",
        calls.len()
    )?;
    for call in calls {
        writeln!(
            out,
            "  {}:{}  {}  candidates={:?}",
            call.location.path,
            call.location.line,
            call.expression.replace('\n', " "),
            call.candidates
        )?;
    }
    writeln!(out, "\nSpawners ({}):", spawners.len())?;
    for s in spawners {
        writeln!(
            out,
            "  {}:{}  {}  spawn_path={:?}",
            s.scene,
            s.line,
            s.node.0,
            s.spawn_path.as_ref().map(|p| &p.0)
        )?;
        for scene in &s.auto_spawn_list {
            writeln!(out, "    {scene}")?;
        }
    }
    writeln!(out, "\nSynchronizers ({}):", synchronizers.len())?;
    for s in synchronizers {
        writeln!(
            out,
            "  {}:{}  {}  root_path={:?}",
            s.scene,
            s.line,
            s.node.0,
            s.root_path.as_ref().map(|p| &p.0)
        )?;
        for p in &s.properties {
            writeln!(
                out,
                "    {}  spawn={:?} mode={:?}",
                p.path.0, p.spawn, p.mode
            )?;
        }
    }
    Ok(())
}

fn unknowns(out: &mut dyn Write, unknowns: &[Unknown]) -> std::io::Result<()> {
    writeln!(out, "\nUnknowns ({}):", unknowns.len())?;
    for u in unknowns {
        if let Some(at) = &u.location {
            write!(out, "  {}:{}  ", at.path, at.line)?;
        } else {
            write!(out, "  ")?;
        }
        writeln!(out, "{}", u.message)?;
    }
    Ok(())
}
