use crate::bus::{BusHandle, Name};
use crate::error::AppError;

pub trait App {
    fn init(&mut self) -> Result<(), AppError>{
        Ok(())
    }
    fn tick(&mut self, bus: &BusHandle) -> Result<(), AppError>;

    fn handle_command(&mut self, _name: &str, _args: &[Name]) -> Result<(), AppError> {
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), AppError>{
        Ok(())
    }

    fn name(&self) -> &'static str{
        "unnamed app"
    }
}