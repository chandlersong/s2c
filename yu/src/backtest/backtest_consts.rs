pub enum PositionDirection {
    Long,
    Short,
}

impl PositionDirection {
    pub fn to_i32(&self) -> i32 {
        match self {
            PositionDirection::Long => 1,
            PositionDirection::Short => -1,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            PositionDirection::Long => "long",
            PositionDirection::Short => "short",
        }
    }
}
