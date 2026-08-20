use crate::bus::BusHandle;
use crate::error::AppError;

pub trait App {
    fn init(&mut self) -> Result<(), AppError>{
        Ok(())
    }
    fn tick(&mut self, bus: &BusHandle) -> Result<(), AppError>;

    fn shutdown(&mut self) -> Result<(), AppError>{
        Ok(())
    }

    fn name(&self) -> &'static str{
        "unnamed app"
    }
}