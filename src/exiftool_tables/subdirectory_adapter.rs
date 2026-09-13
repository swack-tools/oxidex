//! Migrate a generated subdirectory edge while its parent still uses a
//! hand-written reader. Location, selection and descent use the same routines
//! as a complete `ProcessExif` walk; this adapter adds no tag knowledge.

use super::{
    DirectoryRule, Emitted, Guard, IfdDir, IfdEntry, IfdTable, IfdTag, accepted_type, cond,
    descend, find_table, locate, resolve,
};

/// One parent directory's shared subdirectory reader. Keep it alive across
/// entries so repeated pointers share the ordinary recursion/duplicate guard.
pub struct SubdirectoryReader {
    guard: Guard,
}

impl Default for SubdirectoryReader {
    fn default() -> Self {
        Self::new()
    }
}

impl SubdirectoryReader {
    #[must_use]
    pub fn new() -> Self {
        Self {
            guard: Guard::new(),
        }
    }

    /// Handle an entry only when every possible selected alternative is a
    /// modeled edge to an enabled binary table. Other entries remain the
    /// caller's responsibility. The parent table need not be enabled: this
    /// does not emit its fields or bypass a child's gates.
    ///
    /// `true` includes a false native condition and unreadable bytes. Those
    /// are handled omissions, and must not fall back to a second producer.
    /// `false` means no state was touched and the entry was not migrated.
    pub fn try_process(
        &mut self,
        table: &'static IfdTable,
        dir: IfdDir<'_>,
        entry: &IfdEntry,
        entry_count: usize,
        ctx: &mut cond::Ctx,
        out: &mut Vec<Emitted>,
    ) -> bool {
        let supported = if let Some(tag) = table.tag(entry.tag_id) {
            supported_edge(tag, false)
        } else if let Some(group) = table.variant_group(entry.tag_id) {
            !group.alternatives.is_empty()
                && group
                    .alternatives
                    .iter()
                    .all(|(_, tag)| supported_edge(tag, true))
        } else {
            false
        };
        if !supported {
            return false;
        }

        let Some(ty) = accepted_type(entry.field_type, table.group0 == "MakerNotes", ctx) else {
            return true;
        };
        let Ok(located) = locate(
            &dir,
            entry,
            ty,
            DirectoryRule::for_table(table, dir.ifd_start, entry_count),
        ) else {
            return true;
        };
        let Some(resolved) = resolve(table, entry, &located, ctx) else {
            return true;
        };
        // supported_edge proved every selectable tag has this edge, before
        // resolve could apply any native condition's state effects.
        if let Some(edge) = &resolved.tag.subdir {
            descend(
                table,
                resolved.tag,
                edge,
                &located,
                &dir,
                ctx,
                &mut self.guard,
                out,
            );
        }
        true
    }
}

fn supported_edge(tag: &IfdTag, variant: bool) -> bool {
    if tag.flags.unknown {
        return false;
    }
    let Some(edge) = &tag.subdir else {
        return false;
    };
    if edge.validate || edge.unwalked.is_some() {
        return false;
    }
    let mut omitted = tag.omitted;
    omitted.subdirectory = false;
    if variant || tag.condition.is_some() {
        omitted.condition = false;
    }
    if omitted.any() {
        return false;
    }
    find_table(edge.module, edge.table).is_some_and(|table| table.enabled())
}
