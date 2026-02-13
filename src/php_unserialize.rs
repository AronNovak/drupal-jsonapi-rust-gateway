use indexmap::IndexMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum PhpValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(IndexMap<PhpArrayKey, PhpValue>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PhpArrayKey {
    Int(i64),
    String(String),
}

impl fmt::Display for PhpArrayKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhpArrayKey::Int(i) => write!(f, "{}", i),
            PhpArrayKey::String(s) => write!(f, "{}", s),
        }
    }
}

#[allow(dead_code)]
impl PhpValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PhpValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PhpValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PhpValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&IndexMap<PhpArrayKey, PhpValue>> {
        match self {
            PhpValue::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&PhpValue> {
        self.as_array()?.get(&PhpArrayKey::String(key.to_string()))
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key)?.as_i64()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PhpUnserializeError {
    #[error("unexpected end of input")]
    UnexpectedEnd,
    #[error("unexpected character '{0}' at position {1}")]
    UnexpectedChar(char, usize),
    #[error("invalid format at position {0}")]
    InvalidFormat(usize),
}

pub fn php_unserialize(input: &[u8]) -> Result<PhpValue, PhpUnserializeError> {
    let mut pos = 0;
    let result = parse_value(input, &mut pos)?;
    Ok(result)
}

fn parse_value(input: &[u8], pos: &mut usize) -> Result<PhpValue, PhpUnserializeError> {
    if *pos >= input.len() {
        return Err(PhpUnserializeError::UnexpectedEnd);
    }

    match input[*pos] {
        b'N' => parse_null(input, pos),
        b'b' => parse_bool(input, pos),
        b'i' => parse_int(input, pos),
        b'd' => parse_float(input, pos),
        b's' => parse_string(input, pos),
        b'a' => parse_array(input, pos),
        b'O' => parse_object_as_array(input, pos),
        ch => Err(PhpUnserializeError::UnexpectedChar(ch as char, *pos)),
    }
}

fn parse_null(input: &[u8], pos: &mut usize) -> Result<PhpValue, PhpUnserializeError> {
    expect(input, pos, b'N')?;
    expect(input, pos, b';')?;
    Ok(PhpValue::Null)
}

fn parse_bool(input: &[u8], pos: &mut usize) -> Result<PhpValue, PhpUnserializeError> {
    expect(input, pos, b'b')?;
    expect(input, pos, b':')?;
    if *pos >= input.len() {
        return Err(PhpUnserializeError::UnexpectedEnd);
    }
    let val = input[*pos] == b'1';
    *pos += 1;
    expect(input, pos, b';')?;
    Ok(PhpValue::Bool(val))
}

fn parse_int(input: &[u8], pos: &mut usize) -> Result<PhpValue, PhpUnserializeError> {
    expect(input, pos, b'i')?;
    expect(input, pos, b':')?;
    let num_str = read_until(input, pos, b';')?;
    let val: i64 = num_str
        .parse()
        .map_err(|_| PhpUnserializeError::InvalidFormat(*pos))?;
    Ok(PhpValue::Int(val))
}

fn parse_float(input: &[u8], pos: &mut usize) -> Result<PhpValue, PhpUnserializeError> {
    expect(input, pos, b'd')?;
    expect(input, pos, b':')?;
    let num_str = read_until(input, pos, b';')?;
    let val: f64 = if num_str == "INF" {
        f64::INFINITY
    } else if num_str == "-INF" {
        f64::NEG_INFINITY
    } else if num_str == "NAN" {
        f64::NAN
    } else {
        num_str
            .parse()
            .map_err(|_| PhpUnserializeError::InvalidFormat(*pos))?
    };
    Ok(PhpValue::Float(val))
}

fn parse_string(input: &[u8], pos: &mut usize) -> Result<PhpValue, PhpUnserializeError> {
    expect(input, pos, b's')?;
    expect(input, pos, b':')?;
    let len_str = read_until(input, pos, b':')?;
    let len: usize = len_str
        .parse()
        .map_err(|_| PhpUnserializeError::InvalidFormat(*pos))?;
    expect(input, pos, b'"')?;
    if *pos + len > input.len() {
        return Err(PhpUnserializeError::UnexpectedEnd);
    }
    let s = String::from_utf8_lossy(&input[*pos..*pos + len]).to_string();
    *pos += len;
    expect(input, pos, b'"')?;
    expect(input, pos, b';')?;
    Ok(PhpValue::String(s))
}

fn parse_array(input: &[u8], pos: &mut usize) -> Result<PhpValue, PhpUnserializeError> {
    expect(input, pos, b'a')?;
    expect(input, pos, b':')?;
    let count_str = read_until(input, pos, b':')?;
    let count: usize = count_str
        .parse()
        .map_err(|_| PhpUnserializeError::InvalidFormat(*pos))?;
    expect(input, pos, b'{')?;

    let mut map = IndexMap::with_capacity(count);
    for _ in 0..count {
        let key = parse_array_key(input, pos)?;
        let value = parse_value(input, pos)?;
        map.insert(key, value);
    }

    expect(input, pos, b'}')?;
    Ok(PhpValue::Array(map))
}

fn parse_object_as_array(input: &[u8], pos: &mut usize) -> Result<PhpValue, PhpUnserializeError> {
    // O:len:"classname":count:{...} — treat as array
    expect(input, pos, b'O')?;
    expect(input, pos, b':')?;
    let len_str = read_until(input, pos, b':')?;
    let len: usize = len_str
        .parse()
        .map_err(|_| PhpUnserializeError::InvalidFormat(*pos))?;
    expect(input, pos, b'"')?;
    *pos += len; // skip class name
    expect(input, pos, b'"')?;
    expect(input, pos, b':')?;
    let count_str = read_until(input, pos, b':')?;
    let count: usize = count_str
        .parse()
        .map_err(|_| PhpUnserializeError::InvalidFormat(*pos))?;
    expect(input, pos, b'{')?;

    let mut map = IndexMap::with_capacity(count);
    for _ in 0..count {
        let key = parse_array_key(input, pos)?;
        let value = parse_value(input, pos)?;
        map.insert(key, value);
    }

    expect(input, pos, b'}')?;
    Ok(PhpValue::Array(map))
}

fn parse_array_key(input: &[u8], pos: &mut usize) -> Result<PhpArrayKey, PhpUnserializeError> {
    if *pos >= input.len() {
        return Err(PhpUnserializeError::UnexpectedEnd);
    }
    match input[*pos] {
        b'i' => {
            let val = parse_int(input, pos)?;
            if let PhpValue::Int(i) = val {
                Ok(PhpArrayKey::Int(i))
            } else {
                Err(PhpUnserializeError::InvalidFormat(*pos))
            }
        }
        b's' => {
            let val = parse_string(input, pos)?;
            if let PhpValue::String(s) = val {
                Ok(PhpArrayKey::String(s))
            } else {
                Err(PhpUnserializeError::InvalidFormat(*pos))
            }
        }
        ch => Err(PhpUnserializeError::UnexpectedChar(ch as char, *pos)),
    }
}

fn expect(input: &[u8], pos: &mut usize, expected: u8) -> Result<(), PhpUnserializeError> {
    if *pos >= input.len() {
        return Err(PhpUnserializeError::UnexpectedEnd);
    }
    if input[*pos] != expected {
        return Err(PhpUnserializeError::UnexpectedChar(
            input[*pos] as char,
            *pos,
        ));
    }
    *pos += 1;
    Ok(())
}

fn read_until(input: &[u8], pos: &mut usize, delimiter: u8) -> Result<String, PhpUnserializeError> {
    let start = *pos;
    while *pos < input.len() && input[*pos] != delimiter {
        *pos += 1;
    }
    if *pos >= input.len() {
        return Err(PhpUnserializeError::UnexpectedEnd);
    }
    let s = String::from_utf8_lossy(&input[start..*pos]).to_string();
    *pos += 1; // skip delimiter
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null() {
        assert_eq!(php_unserialize(b"N;").unwrap(), PhpValue::Null);
    }

    #[test]
    fn test_bool() {
        assert_eq!(php_unserialize(b"b:1;").unwrap(), PhpValue::Bool(true));
        assert_eq!(php_unserialize(b"b:0;").unwrap(), PhpValue::Bool(false));
    }

    #[test]
    fn test_int() {
        assert_eq!(php_unserialize(b"i:42;").unwrap(), PhpValue::Int(42));
        assert_eq!(php_unserialize(b"i:-1;").unwrap(), PhpValue::Int(-1));
    }

    #[test]
    fn test_float() {
        assert_eq!(php_unserialize(b"d:3.14;").unwrap(), PhpValue::Float(3.14));
    }

    #[test]
    fn test_string() {
        assert_eq!(
            php_unserialize(b"s:5:\"hello\";").unwrap(),
            PhpValue::String("hello".to_string())
        );
    }

    #[test]
    fn test_array() {
        let input = b"a:2:{s:3:\"foo\";s:3:\"bar\";s:3:\"baz\";i:42;}";
        let result = php_unserialize(input).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(result.get_str("foo").unwrap(), "bar");
        assert_eq!(result.get_i64("baz").unwrap(), 42);
    }
}
