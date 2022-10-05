use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    str::from_utf8,
    thread::sleep,
    time::Duration,
};

use nom::{
    branch::alt,
    bytes::streaming::{tag_no_case, take_till1, take_while1, take_while_m_n},
    character::{complete::space1, is_digit, is_space, streaming::hex_digit1},
    combinator::map,
    multi::{count, length_count},
    sequence::{terminated, tuple},
    IResult,
};

pub struct Lidar {
    socket: TcpStream,
    unparsed: Vec<u8>,
}

impl Lidar {
    pub fn connect(address: impl ToSocketAddrs) -> Self {
        let mut socket = TcpStream::connect(address).unwrap();
        let mut unparsed = vec![];
        set_access_mode(&mut socket, &mut unparsed);
        start_measurement(&mut socket, &mut unparsed);
        set_active_applications(
            &mut socket,
            &mut unparsed,
            Application::Field,
            ApplicationActivation::Disabled,
        );
        set_active_applications(
            &mut socket,
            &mut unparsed,
            Application::Ranging,
            ApplicationActivation::Enabled,
        );
        run(&mut socket, &mut unparsed);
        wait_for_ready(&mut socket, &mut unparsed);
        Self { socket, unparsed }
    }

    pub fn poll_data(&mut self) -> Vec<usize> {
        get_scan_data(&mut self.socket, &mut self.unparsed)
    }
}

fn wait_for_ready(socket: &mut TcpStream, unparsed: &mut Vec<u8>) {
    let mut unsuccessful_readies = 0;
    loop {
        Request::DeviceState.write_to(socket);
        let response = get_next_response(socket, unparsed);
        if let Response::DeviceState(DeviceState::Ready) = response {
            break;
        }
        if let Response::DeviceState(DeviceState::Error) = response {
            panic!("BROKEN!");
        }
        dbg!(&response);
        unsuccessful_readies += 1;
        if unsuccessful_readies >= 60 {
            unsuccessful_readies = 0;
            println!("restarting measurement...");
            stop_measurement(socket, unparsed);
            start_measurement(socket, unparsed);
        }
        sleep(Duration::from_secs(1));
    }
}

fn set_access_mode(socket: &mut TcpStream, unparsed: &mut Vec<u8>) {
    Request::SetAccessMode.write_to(socket);
    let response = get_next_response(socket, unparsed);
    match response {
        Response::SetAccessMode(SetAccessModeResult::Error) => panic!("BROKEN!"),
        Response::SetAccessMode(SetAccessModeResult::Success) => return,
        _ => panic!("wut?!"),
    }
}

fn start_measurement(socket: &mut TcpStream, unparsed: &mut Vec<u8>) {
    Request::StartMeasurement.write_to(socket);
    let response = get_next_response(socket, unparsed);
    match response {
        Response::StartMeasurement(StartMeasurementResult::Success) => return,
        Response::StartMeasurement(StartMeasurementResult::NotAllowed) => panic!("BROKEN!"),
        _ => panic!("wut?!"),
    }
}

fn stop_measurement(socket: &mut TcpStream, unparsed: &mut Vec<u8>) {
    Request::StopMeasurement.write_to(socket);
    let response = get_next_response(socket, unparsed);
    match response {
        Response::StopMeasurement(StopMeasurementResult::Success) => return,
        Response::StopMeasurement(StopMeasurementResult::NotAllowed) => panic!("BROKEN!"),
        _ => panic!("wut?!"),
    }
}

fn set_active_applications(
    socket: &mut TcpStream,
    unparsed: &mut Vec<u8>,
    application: Application,
    activation: ApplicationActivation,
) {
    Request::SetApplicationActivation(application, activation).write_to(socket);
    let response = get_next_response(socket, unparsed);
    match response {
        Response::SetActiveApplications => return,
        _ => panic!("wut?!"),
    }
}

fn run(socket: &mut TcpStream, unparsed: &mut Vec<u8>) {
    Request::Run.write_to(socket);
    let response = get_next_response(socket, unparsed);
    match response {
        Response::Run(RunResult::Error) => panic!("BROKEN!"),
        Response::Run(RunResult::Success) => return,
        _ => panic!("wut?!"),
    }
}

fn get_scan_data(socket: &mut TcpStream, unparsed: &mut Vec<u8>) -> Vec<usize> {
    Request::ScanData.write_to(socket);
    let response = get_next_response(socket, unparsed);
    match response {
        Response::ScanData { values } => values,
        _ => panic!("wut?!"),
    }
}

fn get_next_response(socket: &mut TcpStream, unparsed: &mut Vec<u8>) -> Response {
    loop {
        let mut buffer = [0; 4096];
        let read_bytes = socket.read(&mut buffer).unwrap();
        unparsed.extend(&buffer[..read_bytes]);
        let (remaining, response) = match response(&unparsed[..]) {
            Ok((remaining, response)) => (remaining, response),
            Err(nom::Err::Incomplete(_)) => continue,
            Err(error) => panic!("Du bist doof weil: {error:?}"),
        };
        *unparsed = remaining.to_vec();
        break response;
    }
}

fn response(input: &[u8]) -> IResult<&[u8], Response> {
    alt((
        map(
            tuple((
                tag_no_case("\x02sRA SCdevicestate "),
                take_while_m_n(1, 1, is_digit),
                tag_no_case("\x03"),
            )),
            |(_, state, _)| {
                Response::DeviceState(match from_utf8(state).unwrap().parse().unwrap() {
                    0 => DeviceState::Busy,
                    1 => DeviceState::Ready,
                    2 => DeviceState::Error,
                    _ => unimplemented!(),
                })
            },
        ),
        map(
            tuple((
                tag_no_case("\x02sAN SetAccessMode "),
                take_while_m_n(1, 1, is_digit),
                tag_no_case("\x03"),
            )),
            |(_, state, _)| {
                Response::SetAccessMode(match from_utf8(state).unwrap().parse().unwrap() {
                    0 => SetAccessModeResult::Error,
                    1 => SetAccessModeResult::Success,
                    _ => unimplemented!(),
                })
            },
        ),
        map(
            tuple((
                tag_no_case("\x02sAN LMCstartmeas "),
                take_while_m_n(1, 1, is_digit),
                tag_no_case("\x03"),
            )),
            |(_, state, _)| {
                Response::StartMeasurement(match from_utf8(state).unwrap().parse().unwrap() {
                    0 => StartMeasurementResult::Success,
                    1 => StartMeasurementResult::NotAllowed,
                    _ => unimplemented!(),
                })
            },
        ),
        map(
            tuple((
                tag_no_case("\x02sAN LMCstopmeas "),
                take_while_m_n(1, 1, is_digit),
                tag_no_case("\x03"),
            )),
            |(_, state, _)| {
                Response::StopMeasurement(match from_utf8(state).unwrap().parse().unwrap() {
                    0 => StopMeasurementResult::Success,
                    1 => StopMeasurementResult::NotAllowed,
                    _ => unimplemented!(),
                })
            },
        ),
        map(tag_no_case("\x02sWA SetActiveApplications\x03"), |_| {
            Response::SetActiveApplications
        }),
        map(
            tuple((
                tag_no_case("\x02sAN Run "),
                take_while_m_n(1, 1, is_digit),
                tag_no_case("\x03"),
            )),
            |(_, state, _)| {
                Response::Run(match from_utf8(state).unwrap().parse().unwrap() {
                    0 => RunResult::Error,
                    1 => RunResult::Success,
                    _ => unimplemented!(),
                })
            },
        ),
        map(
            tuple((
                tag_no_case("\x02sRA LMDscandata "),
                count(tuple((take_till1(is_space), space1)), 17),
                tag_no_case("1 "),
                count(tuple((take_till1(is_space), space1)), 5),
                length_count(
                    map(terminated(hex_digit1, space1), |amount_of_data| {
                        usize::from_str_radix(from_utf8(amount_of_data).unwrap(), 16).unwrap()
                    }),
                    map(terminated(hex_digit1, space1), |value| {
                        usize::from_str_radix(from_utf8(value).unwrap(), 16).unwrap()
                    }),
                ),
                take_while1(is_not_end),
                tag_no_case("\x03"),
            )),
            |(_, _, _, _, values, _, _)| Response::ScanData { values },
        ),
    ))(input)
}

fn is_not_end(character: u8) -> bool {
    character != 0x03
}

#[derive(Debug)]
enum Request {
    DeviceState,
    SetAccessMode,
    StartMeasurement,
    StopMeasurement,
    SetApplicationActivation(Application, ApplicationActivation),
    Run,
    ScanData,
}

impl Request {
    fn write_to(&self, socket: &mut TcpStream) {
        match self {
            Request::DeviceState => socket.write_all(b"\x02sRN SCdevicestate\x03").unwrap(),
            Request::SetAccessMode => socket
                .write_all(b"\x02sMN SetAccessMode 03 F4724744\x03")
                .unwrap(),
            Request::StartMeasurement => socket.write_all(b"\x02sMN LMCstartmeas\x03").unwrap(),
            Request::StopMeasurement => socket.write_all(b"\x02sMN LMCstopmeas\x03").unwrap(),
            Request::SetApplicationActivation(application, activation) => {
                let mut request = vec![];
                write!(
                    request,
                    "\x02sWN SetActiveApplications 1 {} {}\x03",
                    match application {
                        Application::Field => "FEVL",
                        Application::Ranging => "RANG",
                    },
                    match activation {
                        ApplicationActivation::Disabled => "0",
                        ApplicationActivation::Enabled => "1",
                    }
                )
                .unwrap();
                socket.write_all(&request).unwrap()
            }
            Request::Run => socket.write_all(b"\x02sMN Run\x03").unwrap(),
            Request::ScanData => socket.write_all(b"\x02sRN LMDscandata\x03").unwrap(),
        }
    }
}

#[derive(Debug)]
enum Application {
    Field,
    Ranging,
}

#[derive(Debug)]
enum ApplicationActivation {
    Enabled,
    Disabled,
}

#[derive(Debug)]
enum Response {
    DeviceState(DeviceState),
    SetAccessMode(SetAccessModeResult),
    StartMeasurement(StartMeasurementResult),
    StopMeasurement(StopMeasurementResult),
    SetActiveApplications,
    Run(RunResult),
    ScanData { values: Vec<usize> },
}

#[derive(Debug)]
enum DeviceState {
    Busy,
    Ready,
    Error,
}

#[derive(Debug)]
enum SetAccessModeResult {
    Error,
    Success,
}

#[derive(Debug)]
enum StartMeasurementResult {
    Success,
    NotAllowed,
}

#[derive(Debug)]
enum StopMeasurementResult {
    Success,
    NotAllowed,
}

#[derive(Debug)]
enum RunResult {
    Error,
    Success,
}
