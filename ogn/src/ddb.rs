use std::collections::HashMap;
use std::io::Read;

use anyhow::{format_err, Result};
use csv::StringRecord;

pub const OGN_DDB_URL: &'static str = "https://ddb.glidernet.org/download/";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    pub id: u32,
    pub typ: char,
    pub model: String,
    pub registration: String,
    pub common_name: String,
    pub tracked: bool,
    pub identified: bool,
}

impl Device {
    pub fn id_str(&self) -> String {
        format!("{:X}", self.id)
    }
}

pub struct DeviceIterator<R: Read> {
    inner: csv::StringRecordsIntoIter<R>,
    field_indexes: FieldIndexes,
}

impl<R: Read> DeviceIterator<R> {
    pub fn new(read: R) -> Result<Self> {
        let reader = csv::ReaderBuilder::new()
            // OGN DDB uses comma
            .delimiter(b',')
            // OGN DDB uses single quotes
            .quote(b'\'')
            .from_reader(read);
        Self::from_csv_reader(reader)
    }

    pub fn from_csv_reader(mut reader: csv::Reader<R>) -> Result<Self> {
        let header = fix_headers(reader.headers()?);
        let field_indexes = FieldIndexes::get_indexes(&header)?;
        Ok(Self {
            inner: reader.into_records(),
            field_indexes,
        })
    }
}

impl<R: Read> Iterator for DeviceIterator<R> {
    type Item = Result<Device>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|sr| {
            sr.map_err(Into::into)
                .and_then(|sr| self.field_indexes.parse_device_record(&sr))
        })
    }
}

pub fn read_database<R: Read>(r: R) -> Result<Vec<Device>> {
    DeviceIterator::new(r)?.collect()
}

pub fn index_by_id(devices: Vec<Device>) -> HashMap<u32, Device> {
    devices.into_iter().map(|d| (d.id.clone(), d)).collect()
}

/// Remove the leading '#' from the headers, because the OGN database adds a '#' to the CSV header
/// as if it's a comment.
fn fix_headers(header: &StringRecord) -> StringRecord {
    header.iter().map(|s| s.trim_start_matches('#')).collect()
}

struct FieldIndexes {
    id: usize,
    typ: usize,
    model: usize,
    registration: usize,
    common_name: usize,
    tracked: usize,
    identified: usize,
}

impl FieldIndexes {
    pub fn get_indexes(header: &StringRecord) -> Result<Self> {
        Ok(Self {
            id: Self::find_field_index(header, "DEVICE_ID")?,
            typ: Self::find_field_index(header, "DEVICE_TYPE")?,
            model: Self::find_field_index(header, "AIRCRAFT_MODEL")?,
            registration: Self::find_field_index(header, "REGISTRATION")?,
            common_name: Self::find_field_index(header, "CN")?,
            tracked: Self::find_field_index(header, "TRACKED")?,
            identified: Self::find_field_index(header, "IDENTIFIED")?,
        })
    }

    pub fn parse_device_record(&self, record: &StringRecord) -> Result<Device> {
        let id = record
            .get(self.id)
            .ok_or_else(|| format_err!("could not get DEVICE_ID"))?;
        let id = u32::from_str_radix(id, 16)?;
        let typ = record
            .get(self.typ)
            .ok_or_else(|| format_err!("could not get DEVICE_TYPE"))?;
        let model = record
            .get(self.model)
            .ok_or_else(|| format_err!("could not get DEVICE_ID"))?;
        let registration = record
            .get(self.registration)
            .ok_or_else(|| format_err!("could not get DEVICE_ID"))?;
        let common_name = record
            .get(self.common_name)
            .ok_or_else(|| format_err!("could not get DEVICE_ID"))?;
        let tracked_str = record
            .get(self.tracked)
            .ok_or_else(|| format_err!("could not get DEVICE_ID"))?;
        let identified_str = record
            .get(self.identified)
            .ok_or_else(|| format_err!("could not get DEVICE_ID"))?;

        let tracked = Self::parse_bool(tracked_str)?;
        let identified = Self::parse_bool(identified_str)?;

        Ok(Device {
            id: id,
            typ: typ
                .chars()
                .next()
                .ok_or_else(|| format_err!("could not get device type \"{}\"", typ))?,
            model: model.to_owned(),
            registration: registration.to_owned(),
            common_name: common_name.to_owned(),
            tracked,
            identified,
        })
    }

    fn parse_bool(b: &str) -> Result<bool> {
        match b {
            "Y" | "y" => Ok(true),
            "N" | "n" => Ok(false),
            _ => Err(format_err!("incorrect boolean \"{}\"", b)),
        }
    }

    fn find_field_index(header: &StringRecord, field: &str) -> Result<usize> {
        header
            .iter()
            .position(|t: &str| t.eq(field))
            .ok_or_else(|| format_err!("could not find field {}", field))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_deserialize_csv() {
        let database = r#"#DEVICE_TYPE,DEVICE_ID,AIRCRAFT_MODEL,REGISTRATION,CN,TRACKED,IDENTIFIED
'F','040893','X-Wing','SW-1234','BS','Y','Y'
'O','04CD01','Drone','HS','HS','Y','N'
'F','06DDC1','LS-32','AA-BCD','A2','N','Y'"#;
        let devices: Vec<Device> = read_database(database.as_bytes()).expect("should parse");

        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].id, 0x040893);
        assert_eq!(devices[0].typ, 'F');
        assert_eq!(devices[0].model, "X-Wing");
        assert_eq!(devices[0].registration, "SW-1234");
        assert_eq!(devices[0].common_name, "BS");
        assert_eq!(devices[0].tracked, true);
        assert_eq!(devices[0].identified, true);
    }
}
