//! Bounded tagged-PDF role lookup. Structure-tree MCIDs override generic /Span
//! tags; custom roles follow RoleMap. Geometry still supplies missing structure.
use crate::limits::Budget;
use crate::{Error, MarkkitBuilder};
use hayro_syntax::Pdf;
use hayro_syntax::object::{Dict, Name, Object, ObjectIdentifier};
use std::collections::{HashMap, HashSet};

pub(super) type Roles = HashMap<(ObjectIdentifier, i32), String>;
pub(super) fn roles(pdf: &Pdf, cfg: &MarkkitBuilder, clock: &Budget) -> Result<Roles, Error> {
    let mut out = HashMap::new();
    let mut root = None;
    for obj in pdf.objects() {
        clock.check_time()?;
        clock.add_nodes(1)?;
        if let Object::Dict(d) = obj
            && super::font::name_of(d.get::<Name<'_>>(b"Type")) == "Catalog"
        {
            root = d.get::<Dict<'_>>(b"StructTreeRoot");
            break;
        }
    }
    let Some(root) = root else {
        return Ok(out);
    };
    let map = root.get::<Dict<'_>>(b"RoleMap");
    struct Walker<'a, 'b> {
        out: &'b mut Roles,
        seen: HashSet<ObjectIdentifier>,
        map: Option<Dict<'a>>,
        clock: &'b Budget,
        depth: usize,
    }
    impl Walker<'_, '_> {
        fn walk(
            &mut self,
            obj: Object<'_>,
            page: Option<ObjectIdentifier>,
            role: &str,
            depth: usize,
        ) -> Result<(), Error> {
            self.clock.check_time()?;
            self.clock.add_nodes(1)?;
            if depth > self.depth {
                return Err(Error::PdfExtract(
                    "PDF structure tree exceeds max_nesting_depth".into(),
                ));
            }
            match obj {
                Object::Number(n) => {
                    let n = n.as_f32();
                    if n.is_finite()
                        && n >= 0.0
                        && n.fract() == 0.0
                        && let Some(page) = page
                    {
                        self.out.insert((page, n as i32), role.into());
                    }
                }
                Object::Array(a) => {
                    for k in a.iter::<Object<'_>>() {
                        self.walk(k, page, role, depth + 1)?;
                    }
                }
                Object::Dict(d) => {
                    if let Some(id) = d.obj_id()
                        && !self.seen.insert(id)
                    {
                        return Ok(());
                    }
                    let page = d.get::<Dict<'_>>(b"Pg").and_then(|d| d.obj_id()).or(page);
                    let mut role = d
                        .get::<Name<'_>>(b"S")
                        .map(|n| super::font::name_of(Some(n)))
                        .unwrap_or_else(|| role.into());
                    if let Some(map) = &self.map
                        && let Some(mapped) = map.get::<Name<'_>>(role.as_bytes())
                    {
                        role = super::font::name_of(Some(mapped));
                    }
                    if role.len() > 256 {
                        return Err(Error::PdfExtract(
                            "PDF structure role exceeds 256 bytes".into(),
                        ));
                    }
                    if let (Some(id), Some(mcid)) = (page, d.get::<i32>(b"MCID")) {
                        self.out.insert((id, mcid), role.clone());
                    }
                    if let Some(k) = d.get::<Object<'_>>(b"K") {
                        self.walk(k, page, &role, depth + 1)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
    }
    if let Some(k) = root.get::<Object<'_>>(b"K") {
        Walker {
            out: &mut out,
            seen: HashSet::new(),
            map,
            clock,
            depth: cfg.limits.nesting_depth(),
        }
        .walk(k, None, "Span", 0)?;
    }
    Ok(out)
}
