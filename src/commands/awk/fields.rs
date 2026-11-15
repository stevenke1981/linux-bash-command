use super::{regexp::Cache, value::Scalar};
use std::rc::Rc;
pub struct Fields {
    record: Vec<u8>,
    record_value: Option<Scalar>,
    values: Option<Vec<Scalar>>,
    separator: Rc<Vec<u8>>,
    paragraph: bool,
    dirty: bool,
    joiner: Option<Vec<u8>>,
}
impl Default for Fields {
    fn default() -> Self {
        Self {
            record: Vec::new(),
            record_value: Some(Scalar::Uninit),
            values: None,
            separator: Rc::new(Vec::new()),
            paragraph: false,
            dirty: false,
            joiner: None,
        }
    }
}
impl Fields {
    pub fn record(&mut self, record: Vec<u8>, separator: impl Into<Rc<Vec<u8>>>, paragraph: bool) {
        self.record_value = None;
        self.record = record;
        self.values = None;
        self.separator = separator.into();
        self.paragraph = paragraph;
        self.dirty = false;
        self.joiner = None;
    }
    fn ensure(&mut self, cache: &mut Cache) -> Result<&mut Vec<Scalar>, String> {
        if self.values.is_none() {
            self.values = Some(
                split(&self.record, &self.separator, self.paragraph, cache)?
                    .into_iter()
                    .map(Scalar::input)
                    .collect(),
            );
        }
        Ok(self.values.as_mut().unwrap())
    }
    pub fn get(
        &mut self,
        index: usize,
        ofs: &[u8],
        convfmt: &[u8],
        cache: &mut Cache,
    ) -> Result<Scalar, String> {
        if index == 0 {
            self.rebuild(ofs, convfmt)?;
            if self.record_value.is_none() {
                self.record_value = Some(Scalar::input(self.record.clone()));
            }
            return Ok(self.record_value.as_ref().unwrap().clone());
        }
        Ok(self
            .ensure(cache)?
            .get(index - 1)
            .cloned()
            .unwrap_or_default())
    }
    pub fn count(&mut self, cache: &mut Cache) -> Result<usize, String> {
        Ok(self.ensure(cache)?.len())
    }
    pub fn set(
        &mut self,
        index: usize,
        value: Scalar,
        fs: &[u8],
        paragraph: bool,
        convfmt: &[u8],
        cache: &mut Cache,
    ) -> Result<(), String> {
        if index == 0 {
            self.record(value.bytes(convfmt)?, fs.to_vec(), paragraph);
            self.record_value = Some(value);
            return Ok(());
        }
        let fields = self.ensure(cache)?;
        if fields.len() < index {
            fields
                .try_reserve(index - fields.len())
                .map_err(|_| "too many fields")?;
            fields.resize(index, Scalar::Uninit);
        }
        fields[index - 1] = value;
        self.dirty = true;
        Ok(())
    }
    pub fn resize(&mut self, length: usize, cache: &mut Cache) -> Result<(), String> {
        let fields = self.ensure(cache)?;
        if length > fields.len() {
            fields
                .try_reserve(length - fields.len())
                .map_err(|_| "too many fields")?;
        }
        fields.resize(length, Scalar::Uninit);
        self.dirty = true;
        Ok(())
    }
    pub(super) fn capture_ofs(&mut self, ofs: &[u8]) {
        if self.dirty {
            self.joiner = Some(ofs.to_vec());
        }
    }
    fn rebuild(&mut self, ofs: &[u8], convfmt: &[u8]) -> Result<(), String> {
        if self.dirty {
            let ofs = self.joiner.as_deref().unwrap_or(ofs);
            self.record.clear();
            for (index, value) in self.values.as_ref().unwrap().iter().enumerate() {
                if index > 0 {
                    self.record.extend_from_slice(ofs);
                }
                self.record.extend_from_slice(&value.bytes(convfmt)?);
            }
            self.record_value = Some(Scalar::string(self.record.clone()));
            self.dirty = false;
        }
        Ok(())
    }
}
pub fn split(
    record: &[u8],
    separator: &[u8],
    paragraph: bool,
    cache: &mut Cache,
) -> Result<Vec<Vec<u8>>, String> {
    if record.is_empty() {
        return Ok(Vec::new());
    }
    if separator == b" " {
        return Ok(record
            .split(|b| matches!(*b, b' ' | b'\t' | b'\n'))
            .filter(|field| !field.is_empty())
            .map(<[u8]>::to_vec)
            .collect());
    }
    if separator.is_empty() {
        return Ok(vec![record.to_vec()]);
    }
    if separator.len() == 1 {
        return Ok(record
            .split(|b| *b == separator[0] || (paragraph && *b == b'\n'))
            .map(<[u8]>::to_vec)
            .collect());
    }
    let source = if paragraph {
        let mut source = b"(".to_vec();
        source.extend_from_slice(separator);
        source.extend_from_slice(b")|\n");
        source
    } else {
        separator.to_vec()
    };
    let pattern = cache.get(&source)?;
    let (mut fields, mut start, mut at) = (Vec::new(), 0, 0);
    while let Some((begin, end)) = pattern.find_nonempty(record, at) {
        fields.push(record[start..begin].to_vec());
        start = end;
        at = end;
    }
    fields.push(record[start..].to_vec());
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_whitespace() {
        assert_eq!(
            split(b"  a\tb  ", b" ", false, &mut Cache::default()).unwrap(),
            [b"a", b"b"]
        );
    }
    #[test]
    fn character_separators_keep_empty_fields() {
        assert_eq!(
            split(b",a,", b",", false, &mut Cache::default()).unwrap(),
            [b"".to_vec(), b"a".to_vec(), b"".to_vec()]
        );
    }
    #[test]
    fn regex_separator_longest() {
        assert_eq!(
            split(b"a::b", b":|::", false, &mut Cache::default()).unwrap(),
            [b"a", b"b"]
        );
    }
    #[test]
    fn paragraph_newlines_also_split() {
        assert_eq!(
            split(b"a,b\nc", b",", true, &mut Cache::default()).unwrap(),
            [b"a", b"b", b"c"]
        );
    }
    #[test]
    fn field_assignment_rebuilds_record() {
        let mut f = Fields::default();
        let mut c = Cache::default();
        f.record(b"a b".to_vec(), b" ".to_vec(), false);
        f.set(3, Scalar::Number(2.0), b" ", false, b"%.6g", &mut c)
            .unwrap();
        assert_eq!(
            f.get(0, b":", b"%.6g", &mut c)
                .unwrap()
                .bytes(b"%.6g")
                .unwrap(),
            b"a:b:2"
        );
    }
    #[test]
    fn changing_fs_does_not_resplit_current_record() {
        let mut f = Fields::default();
        let mut c = Cache::default();
        f.record(b"a,b c".to_vec(), b",".to_vec(), false);
        assert_eq!(f.count(&mut c).unwrap(), 2);
        f.resize(1, &mut c).unwrap();
        assert_eq!(
            f.get(0, b" ", b"%.6g", &mut c)
                .unwrap()
                .bytes(b"%.6g")
                .unwrap(),
            b"a"
        );
    }
}
