use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};

use failure::Error;

use crate::bolt;
use crate::bolt::Value;
use crate::error::ValueError;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Relationship {
    pub(crate) rel_identity: i64,
    pub(crate) start_node_identity: i64,
    pub(crate) end_node_identity: i64,
    pub(crate) rel_type: String,
    pub(crate) properties: HashMap<String, Value>,
}

impl Relationship {
    pub fn new(
        rel_identity: i64,
        start_node_identity: i64,
        end_node_identity: i64,
        rel_type: String,
        properties: HashMap<String, impl Into<Value>>,
    ) -> Self {
        Self {
            rel_identity,
            start_node_identity,
            end_node_identity,
            rel_type,
            properties: properties.into_iter().map(|(k, v)| (k, v.into())).collect(),
        }
    }
}

impl TryFrom<bolt::value::Relationship> for Relationship {
    type Error = Error;

    fn try_from(bolt_rel: bolt::value::Relationship) -> Result<Self, Self::Error> {
        Ok(Relationship {
            rel_identity: i64::try_from(*bolt_rel.rel_identity)?,
            start_node_identity: i64::try_from(*bolt_rel.start_node_identity)?,
            end_node_identity: i64::try_from(*bolt_rel.end_node_identity)?,
            rel_type: String::try_from(*bolt_rel.rel_type)?,
            properties: (*bolt_rel.properties).try_into()?,
        })
    }
}

impl TryFrom<Value> for Relationship {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Relationship(rel) => Ok(Relationship::try_from(rel)?),
            _ => Err(ValueError::InvalidConversion(value).into()),
        }
    }
}
