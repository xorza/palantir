use crate::*;

#[derive(Debug, Default, Clone, Copy)]
pub enum Align {
    #[default]
    Stretch,
    Start,
    Center,
    End,    
}


#[derive(Debug, Default, Clone, Copy)]
pub enum Size{
    #[default]
    Auto,
    Fixed(f32),
}

