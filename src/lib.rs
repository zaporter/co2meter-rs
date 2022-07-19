
//! Rust wrapper for co2meter 
//! 
//! This is a very basic rust implementation of the co2meter python package by Vladimir Filimonov (<https://github.com/vfilimonov/co2meter>)
//! 
//! It currently has support for basic monitor reading but lacks the advanced functionality of the
//! other python version like homekit integration and a display server. There is no reason to add
//! this. 
//!
//! # Features
//! `serde` : Enable serde Serialize and Deserialze derives for [CO2Reading] and [CO2MonitorInfo]
//! 
//! # Getting Started
//!
//! ```rust
//! let mut co2 = CO2Monitor::default()?;
//! let result = co2.read_data(true, 50)?;
//! dbg!(result);
//! ```
//!
//! # Get info about your co2 monitor: 
//! ```rust
//! let co2 = CO2Monitor::default()?;
//! let info = co2.get_info();
//! dbg!(info);
//! ```
//! # Specify which co2 monitor you want to read from: 
//! ```rust
//! let interface_path = "...".to_owned()// Mine is "1-13:1.0"
//! let co2 = CO2Monitor::new(false, Some(interface_path))?;
//! let info = co2.get_info();
//! dbg!(info);
//! ```
//!

use std::error::Error;

use chrono::{DateTime, Utc, NaiveDateTime, Local};
use hidapi::{HidApi, DeviceInfo, HidDevice};

const CO2MON_HID_VENDOR_ID : u16 = 0x04d9;
const CO2MON_HID_PRODUCT_ID : u16 = 0xa052;
const CO2MON_MAGIC_WORD :  &str = "Htemp99e";
// CO2MON magic table?
//
const CODE_END_MESSAGE : u8 = 0x0D;
const CODE_CO2 : u8 = 0x50;
const CODE_TEMPERATURE : u8 = 0x42;


fn convert_temperature_to_celcius(temp : u16) -> f32 {
    // goes in increments of 1/16th of a degree kelvin
    temp as f32 * 0.0625 - 273.15
}

fn list_to_u64(x: &[u8]) -> u64 {
    x[7] as u64 +
    ((x[6] as u64) << 8) +
    ((x[5] as u64) << 16) +
    ((x[4] as u64) << 24) +
    ((x[3] as u64) << 32) +
    ((x[2] as u64) << 40) +
    ((x[1] as u64) << 48) +
    ((x[0] as u64) << 56) 
}
fn u64_to_list(x:u64)->[u8;8]{
    let mut list = [0_u8;8];
    list[0] = ((x >> 56) & 0xFF) as u8; 
    list[1] = ((x >> 48) & 0xFF) as u8; 
    list[2] = ((x >> 40) & 0xFF) as u8; 
    list[3] = ((x >> 32) & 0xFF) as u8; 
    list[4] = ((x >> 24) & 0xFF) as u8; 
    list[5] = ((x >> 16) & 0xFF) as u8; 
    list[6] = ((x >> 8) & 0xFF) as u8; 
    list[7] = (x & 0xFF) as u8; 
    list
}
fn get_magic_word() -> [u8;8]{
    let mut list = [0_u8;8];
    let bytes = CO2MON_MAGIC_WORD.bytes();
    let mut i = 0;
    for byte in bytes {
        list[i] = (byte << 4)  | (byte >> 4);
        i+=1;
    }
    list
}
/// A simple struct for return values.  
///
/// If you enable the `serde` feature then this also derives Serialize and Deserialize
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CO2Reading{
    pub time: Option<DateTime<Local>>,
    pub co2_ppm: u32,
    pub temp_c: f32,
}
/// A simple struct to display information about the device
///
/// If you enable the `serde` feature then this also derives Serialize and Deserialize
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug,Clone)]
pub struct CO2MonitorInfo{
    pub vendor_id : u16,
    pub product_id : u16,
    pub path: String,
    pub manufacturer : String,
    pub product_name: String,
    pub serial_no: String,
}
/// The main class to interact with. Instantiating this class can fail as it creates a device
/// connection when it is created.
///
///
pub struct CO2Monitor{
    bypass_decrypt : bool,
    hid : HidApi,
    device_info : DeviceInfo,
    device : Option<HidDevice>,
    magic_table : [u8;8],
}
impl CO2Monitor {
    /// This is the default way to create a CO2Monitor that you will most certainly use. 
    /// It does not bypass decryption and it assumes that grabs the first co2 monitor it sees. Do
    /// not use this if you have multiple co2 monitors on your computer. 
    ///
    /// Equivalent to CO2Monitor::new(false, None). 
    /// 
    pub fn default() -> Result<CO2Monitor, Box<dyn Error>> {
        Self::new(false, None)
    }
    /// Use this if you know you need to bypass decryption (try to do this if the package is not
    /// working. Apparently some models don't have the encryption) or if you need to specify one of
    /// the multiple CO2 monitors you have on your system.  
    pub fn new(bypass_decrypt: bool, interface_path: Option<String>) -> Result<CO2Monitor, Box<dyn Error>> {
        let hid = HidApi::new()?;
        let device_info = Self::find_device(&hid, interface_path).ok_or("Unable to find the hid device")?;


        Ok(CO2Monitor {
            bypass_decrypt,
            hid,
            device_info,
            device:None,
            magic_table : [0_u8;8],
        })
    }
    /// Return a [CO2MonitorInfo] about the device
    pub fn get_info(&self) -> CO2MonitorInfo {
        CO2MonitorInfo { 
            vendor_id: self.device_info.vendor_id(),
            product_id: self.device_info.product_id(),
            path: String::from(self.device_info.path().to_str().unwrap_or("Error")),
            manufacturer: String::from(self.device_info.manufacturer_string().unwrap_or("None provided")),
            product_name: String::from(self.device_info.product_string().unwrap_or("None provided")),
            serial_no: String::from(self.device_info.serial_number().unwrap_or("None provided")) 
        }
    }
    // find the correct co2 monitor. Used in CO2Monitor::new(..)
    fn find_device(hid: &HidApi, interface_path: Option<String>) -> Option<DeviceInfo>{
        for device in hid.device_list(){
            //println!("{:04x}:{:04x}", device.vendor_id(), device.product_id());
            if device.vendor_id() == CO2MON_HID_VENDOR_ID &&
                device.product_id() == CO2MON_HID_PRODUCT_ID {
                // If we are supplied a path, ensure that we skip unmatched ones
                if interface_path.is_some() &&
                    (device.path().to_str().unwrap() != interface_path.as_ref().unwrap().as_str()){
                        continue;
                }
                return Some(device.clone());
            }
        }
        None
    } 
    // open the connection to the device. Assumes that there is no open connection. 
    fn hid_open(&mut self, send_magic_tables : bool) -> Result<(), Box<dyn Error>>{
        assert!(self.device.is_none());
        self.device = Some(self.device_info.open_device(&self.hid)?);
        if send_magic_tables{
            self.device.as_ref().ok_or("No device found to send tables to. Strange.")?.send_feature_report(&self.magic_table)?;
        }
        Ok(())
    }
    // close the connection to the device. Assumes that a connection is already open.
    fn hid_close(&mut self) -> Result<(), Box<dyn Error>>{
        assert!(self.device.is_some());
        self.device = None; // This should call the destructor and close it
        Ok(())
    }
    // Read raw data from the device
    fn hid_read(&mut self) -> Result<[u8;8], Box<dyn Error>>{
        let mut data : [u8;8] = [0;8];
        self.device.as_ref().ok_or("Device is not opened. Call hid_open before hid_read()")?.read(&mut data)?;
        Ok(self.decrypt(data))
    }
    // decrypt the message (used inside hid_read(..))
    fn decrypt(&self, mut data : [u8;8]) -> [u8;8] {
        if self.bypass_decrypt{
            return data;
        }
        // rearrange data and turn into u64
        let rearranged_data : [u8;8] = [
            data[2],
            data[4],
            data[0],
            data[7],
            data[1],
            data[6],
            data[5],
            data[3]
        ];
        let message = list_to_u64(&rearranged_data);
        // XOR with magic table
        let mut result = message ^ list_to_u64(&self.magic_table);
        // cyclic shift by 3 to the right
        result = (result >> 3) | (result << 61);
        let result_list = u64_to_list(result);
        // They really should enable the array_zip feature... Really stupid that they haven't
        let magic_word = get_magic_word();
        let mut i = 0;
        result_list.map(|r| r.wrapping_sub(magic_word[{i+=1;i-1}]))

    }
    // figure out if the message is about co2 or temp
    fn decode_message(&self, msg : [u8;8]) -> (Option<u32>,Option<f32>){
        // verify end of the message is intact
        if msg[5]!=0 || msg[6]!=0 || msg[7] !=0 || msg[4]!= CODE_END_MESSAGE{
            return (None, None);
        }
        // verify checksum
        if (msg[0].wrapping_add(msg[1]).wrapping_add(msg[2])) != msg[3]{
            return (None,None);
        }
        let value : u16 = ((msg[1] as u16) << 8) | msg[2] as u16;
        match msg[0] {
            CODE_CO2 => (Some(value as u32), None),
            CODE_TEMPERATURE => (None, Some(convert_temperature_to_celcius(value))),
            _ =>(None,None),
        }
    }
    fn read_data_inner(&mut self, record_time: bool, max_requests: u32) -> Result<CO2Reading, Box<dyn Error>>{
        let mut co2 : Option<u32> = None;
        let mut temp : Option<f32> = None;
        let mut request_num = 0;
        // XOR, keep going until both the co2 and temp are Some(..)
        while (request_num < max_requests) ^ (co2.is_some() && temp.is_some()) {
            let data = self.hid_read()?;
            let message = self.decode_message(data);
            match message {
                (co2_val,None) => {co2 = co2_val},
                (None, temp_val) => {temp = temp_val},
                _ => {},
            }
            request_num += 1;
        }
        Ok(CO2Reading {
            time : if record_time {Some(chrono::offset::Local::now())} else {None},
            co2_ppm: co2.ok_or("Unable to read the co2 in the allotted number of requests")?,
            temp_c : temp.ok_or("Unable to read the temperature in the allotted number of requests")?,
        })

    }
    /// Returns a [CO2Reading] if successful. 
    /// 
    /// `record_time` should be `true` if you want the `time` value in the result to be set.
    /// Otherwise it will be left as `None`. 
    /// 
    /// `max_requests` specifies the number of times to poll the device. A reccomeneded value is
    /// `50`
    ///
    pub fn read_data(&mut self, record_time: bool, max_requests: u32) -> Result<CO2Reading, Box<dyn Error>>{
        self.hid_open(true)?;
        let result = self.read_data_inner(record_time, max_requests);
        self.hid_close()?;
        result
    }
}


#[cfg(test)]
mod tests{
    use crate::*;

    #[test]
    fn compilation() {
        assert_eq!(1+1,2);
    }
    #[test]
    fn find_device() {
        let co2 = CO2Monitor::default().unwrap();
        assert_eq!(co2.device_info.vendor_id(), CO2MON_HID_VENDOR_ID);
        assert_eq!(co2.device_info.product_id(), CO2MON_HID_PRODUCT_ID);
    }
    #[test]
    fn read_message(){
        let mut co2 = CO2Monitor::default().unwrap();
        let result = co2.read_data(true, 50);
        dbg!(result);
    }
    #[test]
    fn get_info_test(){
        let co2 = CO2Monitor::default().unwrap();
        let info = co2.get_info();
        dbg!(info);

    }

}
