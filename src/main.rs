use dotenv;
use serde::{Serialize, Deserialize};
use urlencoding::encode;
use std::{fs, io, env};
use std::collections::HashMap;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Stylize, Color, Style},
    symbols::{Marker},
    text::{Line, Text},
    widgets::{Block, Paragraph, Borders, Wrap, Cell, Row, Table, Padding, Axis, Chart, GraphType, Dataset},
    prelude::{Alignment},
    DefaultTerminal, Frame,
};
use chrono::{NaiveDate, Datelike};
use std::path::{PathBuf};
use dirs;
use ureq::Agent;
use include_dir::{include_dir, Dir};
use clap::{Parser, Subcommand};

static MOON_PHASE_ART_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/moon-phase-art");


#[derive(Parser, Debug)]
#[command(version, about = "A free, simple weather TUI that pulls data without the need for an API key, account, or subscription")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Allows you to edit a configuration setting
    Edit {
        /// Your current timezone (e.g. America/Chicago, America/New_York, etc)
        #[clap(short, long)]
        timezone: Option<String>,
        
        /// Latitude (e.g. 35.9295) 
        #[clap(short, long)]
        lat: Option<String>,

        /// Longitude (e.g. -83.8906)
        #[clap(short='o', long)]
        long: Option<String>,

        /// Your State code (e.g. TN, KY, PA)
        #[clap(short, long)]
        state: Option<String>,

        /// Your County/Zone code as specified by NOAA (e.g. TNZ069 - More info here: https://wiki.weather-watch.com/index.php/NOAA_US_County_and_Zone_Codes )
        #[clap(short, long)]
        zone: Option<String>
    }
}


/// Configuration Parameters
#[derive(Debug)]
struct ConfigParams {
    timezone: Option<String>,
    lat: Option<String>,
    long: Option<String>,
    state: Option<String>,
    zone: Option<String>
}


/// Single day of weather forecast from NWS
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct NwsPeriod {
    number: i32,
    name: String,
    detailed_forecast: String
}

/// Contains the next 7 days of morning/night weather
#[derive(Serialize, Deserialize, Debug)]
struct NwsProperties {
    periods: Vec<NwsPeriod>
}

/// Forecast from NWS
#[derive(Serialize, Deserialize, Debug)]
struct NwsForecast {
    properties: NwsProperties
}

/// Daily forecast data
#[derive(Serialize, Deserialize, Debug)]
struct OpenMeteoTimeAndCode {
    time: Vec<String>,
    weather_code: Vec<i32>,
    temperature_2m_max: Vec<f32>,
    temperature_2m_min: Vec<f32>,
    apparent_temperature_max: Vec<f32>,
    apparent_temperature_min: Vec<f32>,
    precipitation_probability_mean: Vec<i32>
}

/// Raw hourly data
#[derive(Serialize, Deserialize, Debug)]
struct OpenMeteoHourlyData {
    time: Vec<String>,
    weather_code: Vec<i32>,
    temperature_2m: Vec<f32>
}

/// Today's weather data
#[derive(Serialize, Deserialize, Debug, Default)]
struct CurrentWeatherData {
    temperature_2m: f32,
    apparent_temperature: f32,
    weather_code: i32
}

/// Combination forecast including daily, hourly, and current
#[derive(Serialize, Deserialize, Debug)]
struct OpenMeteoRawForecast {
    daily: OpenMeteoTimeAndCode,
    hourly: OpenMeteoHourlyData,
    current: CurrentWeatherData
}

/// Single day/weather condition 
/// NOTE: Temperature values already include a degree symbol (U+00B0)
#[derive(Serialize, Deserialize, Debug)]
struct OpenMeteoPeriod {
    date: String,
    weather: String,
    temperature_max: String,
    temperature_min: String,
    apparent_temperature_max: String,
    apparent_temperature_min: String,
    precipitation_probability: String,
}

/// Forecast data by the hour
#[derive(Serialize, Deserialize, Debug)]
struct OpenMeteoHourly {
    datetime: String,
    temperature: String,
    weather: String
}

/// Final, reformatted forecast with daily and current weather
#[derive(Serialize, Deserialize, Debug, Default)]
struct OpenMeteoForecast {
    periods: Vec<OpenMeteoPeriod>,
    current: CurrentWeatherData,
    hourly: Vec<OpenMeteoHourly>
}

/// Moon phase data for a given date
#[derive(Serialize, Deserialize, Debug)]
struct MoonPhase {
    date: String,
    phase: String,
    illumination: String
}

/// Raw phase data from ViewBits
#[derive(Serialize, Deserialize, Debug)]
struct RawMoonPhaseData {
    phases: Vec<MoonPhase>
}





/// Create the "Right Now" weather table
fn create_right_now_table(forecast: &OpenMeteoForecast) -> Table {
    let widths = [
        Constraint::Length(15),
        Constraint::Fill(1),
    ];

    let rows = [
        Row::new(vec![
            Cell::from("Current Temp:"),
            Cell::from(Text::from(format!("{}\u{00B0}", forecast.current.temperature_2m)).right_aligned()),
        ]),
        Row::new(vec![
            Cell::from("Feels Like:"),
            Cell::from(Text::from(format!("{}\u{00B0}", forecast.current.apparent_temperature)).right_aligned()),
        ]), 
        Row::new(vec![
            Cell::from("High:"),
            Cell::from(Text::from(format!("{}", forecast.periods[0].temperature_max)).right_aligned()),
        ]),
        Row::new(vec![
            Cell::from("Low:"),
            Cell::from(Text::from(format!("{}", forecast.periods[0].temperature_min)).right_aligned()),
        ]),
        Row::new(vec![
            Cell::from("Weather Summary:"),
            Cell::from(Text::from(format!("{}", forecast.periods[0].weather)).right_aligned()),
        ]),
        Row::new(vec![
            Cell::from("Chance of Rain:"),
            Cell::from(Text::from(format!("{}%", forecast.periods[0].precipitation_probability)).right_aligned()),
        ]),
    ];


    return Table::new(rows, widths).column_spacing(1)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .padding(Padding::uniform(1))
                    .title(Line::from(" Right Now ").light_blue().centered().bold())
            );
}



/// Renders the scatterplot to show the temperature for the rest of the current day
fn render_temperature_scatterplot(frame: &mut Frame, area: Rect, hourly: &Vec<OpenMeteoHourly>) {
    let mut today_hourly: [(f64, f64); 24] = [(0., 0.); 24];
    let mut count: usize = 0;
    for i in hourly {
        let time_split = i.datetime.split("T");
        let time_pieces = time_split.collect::<Vec<_>>();
        let hour_split = time_pieces[1].split(":");
        let hour_pieces = hour_split.collect::<Vec<_>>();
        let hour_as_float = hour_pieces[0].parse::<f64>().unwrap();
        let mut temp_clone = i.temperature.clone();
        temp_clone.pop();
        let temp_as_float = temp_clone.parse::<f64>().unwrap();
        today_hourly[count] = (hour_as_float, temp_as_float);
        count += 1;
        if count == 24 {
            break;
        }
    }


    let dataset = Dataset::default()
            .marker(Marker::Dot)
            .graph_type(GraphType::Scatter)
            .style(Style::new().yellow())
            .data(&today_hourly);

    let chart = Chart::new(vec!(dataset))
        .block(Block::bordered().title(Line::from(" Today's Temps ").cyan().centered().bold()))
        .y_axis(
            Axis::default()
                .title("Temp (\u{00B0}F)")
                .bounds([0., 120.])
                .style(Style::default().fg(Color::Gray))
                .labels(["0", "30", "60", "90", "120"]),
        )
        .x_axis(
            Axis::default()
                .title("Time (HH:MM)")
                .bounds([0., 23.])
                .style(Style::default().fg(Color::Gray))
                .labels(["00:00", "06:00", "12:00", "18:00", "23:00"]),
        )
        .hidden_legend_constraints((Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)));

    frame.render_widget(chart, area);
}



/// Creates the cards for the 4-cast section
fn create_weather_card(period: &OpenMeteoPeriod) -> Table {
        let widths = [
            Constraint::Length(15),
            Constraint::Fill(1)
        ];


        let rows = [
            Row::new(vec![
                Cell::from("High:"),
                Cell::from(Text::from(format!("{}", period.temperature_max)).right_aligned()),
            ]),
            Row::new(vec![
                Cell::from("Low:"),
                Cell::from(Text::from(format!("{}", period.temperature_min)).right_aligned()),
            ]),
            Row::new(vec![
                Cell::from("Weather:"),
                Cell::from(Text::from(format!("{}", period.weather)).right_aligned()),
            ]),
            Row::new(vec![
                Cell::from("Chance of Rain:"),
                Cell::from(Text::from(format!("{}%", period.precipitation_probability)).right_aligned()),
            ]),
        ];


        let day = get_day_from_date(&period.date);

        return Table::new(rows, widths).column_spacing(1)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .padding(Padding::new(2,2,3,0))
                        .title(Line::from(format!(" ({}) {} ", day, period.date)).centered().bold())
                );
}



/// Returns day (Monday, Tuesday, etc) for given date (YYYY-MM-DD)
fn get_day_from_date(date: &String) -> String {
    let date_pieces: Vec<&str> = date.split('-').collect();
    // Parse the day, month, and year as integers
    let year: i32 = date_pieces[0].parse().expect("Invalid year");
    let month: u32 = date_pieces[1].parse().expect("Invalid month");
    let day: u32 = date_pieces[2].parse().expect("Invalid day");

    // Create a NaiveDate object with the provided input using from_ymd_opt
    match NaiveDate::from_ymd_opt(year, month, day) {
        Some(date) => {
            return date.weekday().to_string();
        }
        None => {
            println!("Invalid date provided. Please ensure the date is valid.");
            return String::from("");
        }
    }
}




/// Application state data
#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct App {
    open_meteo_forecast: OpenMeteoForecast,
    todays_weather_description: String,
    moon_phase_art: String,
    exit: bool
}

/// Main Ratatui app for Raijin
impl App {
    /// Runs the application's main loop until the user quits
    fn run(&mut self, terminal: &mut DefaultTerminal, forecast: OpenMeteoForecast, today: String, moon_phase_art: String) -> io::Result<()> {
        self.open_meteo_forecast = forecast;
        self.todays_weather_description = today;
        self.moon_phase_art = moon_phase_art;
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        use Constraint::{Percentage, Ratio};

        let vertical = Layout::vertical([Percentage(50), Percentage(50)]);
        let [today_area, forecast_area] = vertical.areas(frame.area());
        
        let horizontal = Layout::horizontal([Ratio(1,3), Ratio(1,3), Ratio(1,3)]);
        let [current, icon, today] = horizontal.areas(today_area);

        let middle = Layout::vertical([Ratio(1,2), Ratio(1,2)]);
        let [mid_top, mid_bottom] = middle.areas(icon);

        let current_weather = Layout::vertical([Ratio(1,2), Ratio(1,2)]);
        let [quick_stats, description] = current_weather.areas(current);
 
        let outer_block = Block::bordered().title(Line::from(" 4-cast ").light_magenta().centered().bold()).padding(Padding::new(0,0,1,0));
        let inner_block = Block::bordered();
        let inner_area = outer_block.inner(forecast_area);

        let upcoming_weather = Layout::horizontal([Ratio(1,4), Ratio(1,4), Ratio(1,4), Ratio(1,4)]);
        let [slot1, slot2, slot3, slot4] = upcoming_weather.areas(inner_area);
        
        frame.render_widget(outer_block, forecast_area);
        frame.render_widget(inner_block, inner_area);

        frame.render_widget(Block::bordered(), mid_top);
        frame.render_widget(Block::new(), mid_bottom);

        // Render the current moon phase for tonight (they store the current moon phase in the
        // third position):
        frame.render_widget(
            Paragraph::new(self.moon_phase_art.clone()).alignment(Alignment::Center)
                .block(
                    Block::new()
                        .title(Line::from(" Tonight's Moon Phase ").light_yellow().centered().bold())
                )
                , mid_top);

        // Render the logo into the middle of the screen
        let logo = include_str!("./logo.txt");
        frame.render_widget(
            Paragraph::new(logo).alignment(Alignment::Center)
                .block(
                    Block::new()
                        .padding(Padding::new(0,0,2,0))
                )
                .style(Style::new().red())
                , mid_bottom);

        // Render the day's full description into the top-left-bottom section
        frame.render_widget(
            Paragraph::new(self.todays_weather_description.clone()).wrap(Wrap { trim: true }).alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(Line::from(" Right Now Details ").light_green().centered().bold())
                        .padding(Padding::uniform(1))
                )
                , description);

        // Render forecast summary details for right now
        frame.render_widget(create_right_now_table(&self.open_meteo_forecast), quick_stats);
        render_temperature_scatterplot(frame, today, &self.open_meteo_forecast.hourly);
        
        // Populate the 4-cast
        for i in 1..5 {
            let mut render_area: Rect = slot1;
            if i == 2 {
                render_area = slot2;
            }
            else if i == 3 {
                render_area = slot3;
            }
            else if i == 4 {
                render_area = slot4;
            }
            frame.render_widget(create_weather_card(&self.open_meteo_forecast.periods[i]), render_area);
        }
    }

    /// Updates the application's state based on user input
    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}



/// Get the forecast for the next 7 days as well as today's weather conditions
/// Using this API: <https://api.open-meteo.com/v1/forecast>
fn get_open_meteo_weather(agent: &Agent, weather_codes: serde_json::Value) -> Result<OpenMeteoForecast, ureq::Error> {
    let latitude = env::var("LATITUDE").unwrap();
    let longitude = env::var("LONGITUDE").unwrap();
    let mut timezone = env::var("TIMEZONE").unwrap().to_string();
    timezone = encode(&timezone).to_string();
    
    let url = format!("https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&daily=temperature_2m_max,temperature_2m_min,apparent_temperature_max,apparent_temperature_min,weather_code,precipitation_probability_mean&hourly=temperature_2m,weather_code&current=temperature_2m,apparent_temperature,weather_code&timezone={}&forecast_days=14&wind_speed_unit=mph&temperature_unit=fahrenheit&precipitation_unit=inch", latitude.to_string(), longitude.to_string(), timezone);

    let json = agent.get(url)
        .call()?
        .body_mut()
        .read_json::<OpenMeteoRawForecast>()?;

    let mut periods: Vec<OpenMeteoPeriod> = Vec::new();
    let mut count: usize = 0;
    for i in &json.daily.time {
        periods.push(OpenMeteoPeriod {
            date: i.to_string(), 
            weather: weather_codes[json.daily.weather_code[count].to_string()].to_string(),
            temperature_max: format!("{}\u{00B0}", json.daily.temperature_2m_max[count].to_string()),
            temperature_min: format!("{}\u{00B0}", json.daily.temperature_2m_min[count].to_string()),
            apparent_temperature_max: format!("{}\u{00B0}", json.daily.apparent_temperature_max[count].to_string()),
            apparent_temperature_min: format!("{}\u{00B0}", json.daily.apparent_temperature_min[count].to_string()),
            precipitation_probability: json.daily.precipitation_probability_mean[count].to_string()
        });
        count += 1;
    }

    let mut hourly: Vec<OpenMeteoHourly> = Vec::new();
    let mut count: usize = 0;
    for i in &json.hourly.time {
        hourly.push(OpenMeteoHourly {
            datetime: i.to_string(),
            temperature: format!("{}\u{00B0}", json.hourly.temperature_2m[count].to_string()),
            weather: weather_codes[json.hourly.weather_code[count].to_string()].to_string()
        });
        count += 1;
    }

    return Ok(OpenMeteoForecast{periods: periods, current: json.current, hourly: hourly});
}



/// Get the morning/night weather for the next 7 days (including today)
/// Using this API: <https://api.weather.gov/>
fn get_nws_weather_periods(agent: &Agent) -> Result<Vec<NwsPeriod>, ureq::Error> {
    let state = env::var("STATE").unwrap();
    let zone = env::var("ZONE").unwrap();
    let url = format!("https://api.weather.gov/zones/{}/{}/forecast", state.to_string(), zone.to_string());
    
    let response = agent.get(url)
        .call()?
        .body_mut()
        .read_json::<NwsForecast>()?;
    
    return Ok(response.properties.periods);
}



/// Get the phases of the moon for today and the next 3 days
/// Using this API: <https://api.viewbits.com/v1/moonphase>
fn get_moon_phases(agent: &Agent, date: String) -> Result<Vec<MoonPhase>, ureq::Error> {
    let url = format!("https://api.viewbits.com/v1/moonphase?startdate={}", date);
    let moon_phases = agent.get(url)
        .call()?
        .body_mut()
        .read_json::<Vec<MoonPhase>>()?;

    return Ok(moon_phases);
}

fn configure() -> Result<(), Box<dyn std::error::Error>> {
    let folder: PathBuf = dirs::config_dir()
        .expect("configure - Could not find config directory")
        .join("Raijin");

    let file = folder.join(".env");

    if !folder.exists() {
        fs::create_dir(folder)?;
    }

    if !file.exists() {
        fs::write(&file, "ZONE=\"TNZ069\"\nSTATE=\"TN\"\nLATITUDE=\"35.9626444\"\nLONGITUDE=\"-83.9167239\"\nTIMEZONE=\"America/New_York\"\n")?;
    }

    return Ok(());
}


/// Update config file with new values
fn update_config(params: ConfigParams) -> Result<(), Box<dyn std::error::Error>> {
    // This is all gross, but I don't really care and just want it to work
    let original_timezone = env::var("TIMEZONE").unwrap().to_string();
    let original_lat = env::var("LATITUDE").unwrap().to_string();
    let original_long = env::var("LONGITUDE").unwrap().to_string();
    let original_state = env::var("STATE").unwrap().to_string();
    let original_zone = env::var("ZONE").unwrap().to_string();

    let mut new_params = HashMap::new();

    if !&params.timezone.is_some() {
        new_params.insert("TIMEZONE", original_timezone);
    } else {
        new_params.insert("TIMEZONE", params.timezone.clone().unwrap().to_string());
    }

    if !&params.lat.is_some() {
        new_params.insert("LAT", original_lat);
    } else {
        new_params.insert("LAT", params.lat.clone().unwrap().to_string());
    }

    if !&params.long.is_some() {
        new_params.insert("LONG", original_long);
    } else {
        new_params.insert("LONG", params.long.clone().unwrap().to_string());
    }

    if !&params.state.is_some() {
        new_params.insert("STATE", original_state);
    } else {
        new_params.insert("STATE", params.state.clone().unwrap().to_string());
    }

    if !&params.zone.is_some() {
        new_params.insert("ZONE", original_zone);
    } else {
        new_params.insert("ZONE", params.zone.clone().unwrap().to_string());
    }

    let file: PathBuf = dirs::config_dir()
        .expect("update_config - Could not find config directory")
        .join("Raijin")
        .join(".env");

    let file_data = format!("ZONE=\"{}\"\nSTATE=\"{}\"\nLATITUDE=\"{}\"\nLONGITUDE=\"{}\"\nTIMEZONE=\"{}\"\n", new_params["ZONE"], new_params["STATE"], new_params["LAT"], new_params["LONG"], new_params["TIMEZONE"]);
    fs::write(&file, file_data)?;

    Ok(())
}



fn main() -> io::Result<()> {
    let _ = configure();
    let file = dirs::config_dir().expect("main - Could not find config directory").join("Raijin").join(".env");
    let _ = dotenv::from_path(&file).expect("main - Could not find .env file");

    let args = Args::parse();

    match &args.command {
        Some(Commands::Edit { timezone, lat, long, state, zone }) => {
            let config_params = ConfigParams {
                timezone: timezone.clone(),
                lat: lat.clone(),
                long: long.clone(),
                state: state.clone(),
                zone: zone.clone()
            };

            let _ = update_config(config_params);
            return Ok(());
        },
        None => {}
    } 

    let data = include_str!("./weather-codes.json");
    let weather_codes: serde_json::Value = serde_json::from_str(&data).expect("JSON was malformed");

    // This is used as part of the thin authentication that the NWS API uses
    // I'm hardcoding it because it doesn't really matter and you won't get blocked even with heavy
    // use (I pinged this thing constantly during development and never hit a limit)
    let user_agent = "Mozilla/5.0 (X11; Linux x86_64; rv:139.0) Gecko/20100101 Firefox/139.0";

    let config = Agent::config_builder()
        .user_agent(user_agent)
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build();

    let agent = Agent::new_with_config(config);
        
    let nws_periods = get_nws_weather_periods(&agent).unwrap();
    let today = nws_periods[0].detailed_forecast.clone();
    let open_meteo_forecast = get_open_meteo_weather(&agent, weather_codes).unwrap();
    let all_moon_phases = get_moon_phases(&agent, open_meteo_forecast.periods[0].date.clone()).unwrap(); 

    let thing = MOON_PHASE_ART_DIR.get_file(format!("{}.txt", all_moon_phases[3].phase)).unwrap();
    let moon_phase_art = thing.contents_utf8().unwrap();
    
    // Initialize the TUI
    let mut terminal = ratatui::init();
    let app_result = App::default().run(&mut terminal, open_meteo_forecast, today, moon_phase_art.to_string());
    // Restore the terminal before we leave
    ratatui::restore();
    app_result
}
