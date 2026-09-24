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
    widgets::{Block, Clear, Paragraph, Borders, Wrap, Cell, Row, Table, Padding, Axis, Chart, GraphType, Dataset},
    prelude::{Alignment},
    DefaultTerminal, Frame,
};
use chrono::{NaiveDate, Datelike, DateTime, TimeZone, Timelike, Local};
use std::path::{PathBuf};
use dirs;
use ureq::Agent;
use include_dir::{include_dir, Dir};
use clap::{Parser, Subcommand};
use std::time::Instant;

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
        zone: Option<String>,

        /// Temperature Units (e.g. F, C - Defaults to F. OpenMeteo doesn't have Kelvin :/ )
        #[clap(short='u', long)]
        temp_unit: Option<String>,

        /// Whether or not you want the default screen when Raijin opens to be the "legacy" screen (e.g. true or false - Defaults to false)
        #[clap(short, long)]
        default_legacy: Option<String>,
    }
}


/// Configuration Parameters
#[derive(Debug)]
struct ConfigParams {
    timezone: Option<String>,
    lat: Option<String>,
    long: Option<String>,
    state: Option<String>,
    zone: Option<String>,
    temp_unit: Option<String>,
    default_legacy: Option<String>,
}



use std::f64::consts::PI;
use std::fmt;

/// Conversion factor from degrees to radians.
pub const DEG_TO_RAD: f64 = PI / 180.0;

/// Conversion factor from radians to degrees.
pub const RAD_TO_DEG: f64 = 180.0 / PI;

/// An angle measured in degrees.
///
/// Provides type safety to prevent mixing degrees with radians in calculations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Degrees(f64);

impl Degrees {
    /// Create an angle from a value in degrees.
    pub fn new(value: f64) -> Self {
        Self(value)
    }

    /// Return the underlying numeric value as a bare `f64`.
    pub fn value(&self) -> f64 {
        self.0
    }

    /// Normalize to 0-360 range
    pub fn normalized(self) -> Self {
        let mut result = self.0 % 360.0;
        if result < 0.0 {
            result += 360.0;
        }
        Self(result)
    }

    /// Sine of the angle.
    pub fn sin(self) -> f64 {
        self.0.to_radians().sin()
    }

    /// Cosine of the angle.
    pub fn cos(self) -> f64 {
        self.0.to_radians().cos()
    }

    /// Tangent of the angle.
    pub fn tan(self) -> f64 {
        self.0.to_radians().tan()
    }
}

impl fmt::Display for Degrees {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}°", self.0)
    }
}

impl From<f64> for Degrees {
    fn from(value: f64) -> Self {
        Self(value)
    }
}

impl From<Degrees> for f64 {
    fn from(deg: Degrees) -> f64 {
        deg.0
    }
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


fn normalize_degrees(angle: f64) -> f64 {
    Degrees::new(angle).normalized().value()
}

fn julian_day<T: TimeZone>(dt: DateTime<T>) -> f64 {
    // Convert to UTC for Julian Day calculation
    let utc_dt = dt.with_timezone(&chrono::Utc);

    let year = utc_dt.year() as f64;
    let month = utc_dt.month() as f64;
    let day = utc_dt.day() as f64
        + utc_dt.hour() as f64 / 24.0
        + utc_dt.minute() as f64 / 1440.0
        + utc_dt.second() as f64 / 86400.0;

    let mut y = year;
    let mut m = month;

    if month <= 2.0 {
        y -= 1.0;
        m += 12.0;
    }

    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();

    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + day + b - 1524.5
}

/// Calculate Julian Century from a Julian Day number.
/// Julian Century is the number of centuries since the J2000.0 epoch (JD 2451545.0),
/// which corresponds to January 1, 2000, 12:00 TT.
fn julian_century(jd: f64) -> f64 {
    (jd - 2451545.0) / 36525.0
}


fn get_sun_and_moon_stuff(t: f64) -> (f64, f64, f64) {
    // Calculate mean elongation of the Moon
    let d = 297.8501921 + t * (445267.1114034 + t * (-0.0018819 + t * (1.0 / 545868.0 + t * (-1.0 / 113065000.0))));
    normalize_degrees(d);

    // Calculate Sun's mean anomaly
    let m = 357.5291092 + t * (35999.0502909 + t * (-0.0001536 + t * (1.0 / 24490000.0)));
    normalize_degrees(m);

    // Calculate Moon's mean anomaly
    let m_prime = 134.9633964 + t * (477198.8675055 + t * (0.0087414 + t * (1.0 / 69699.0 + t * (-1.0 / 14712000.0))));
    normalize_degrees(m_prime);

    return (d * DEG_TO_RAD, m * DEG_TO_RAD, m_prime * DEG_TO_RAD);
}

/// Calculate phase angle
fn get_phase_angle() -> f64 {
    let jd = julian_day(Local::now());
    let t = julian_century(jd);

    let (d, m, m_prime) = get_sun_and_moon_stuff(t);

    // Illumination angle (0° = full moon, 180° = new moon)
    let illum_angle = 180.0 - d * RAD_TO_DEG - 6.289 * m_prime.sin() + 2.100 * m.sin()
        - 1.274 * (2.0 * d - m_prime).sin()
        - 0.658 * (2.0 * d).sin()
        - 0.214 * (2.0 * m_prime).sin()
        - 0.110 * d.sin();


    // Convert to orbital phase angle (0° = new moon, 180° = full moon)
    return normalize_degrees(180.0 - illum_angle);
}

/// Return the moon phase name given the phase angle
fn phase_name(phase_angle: f64) -> &'static str {
    match phase_angle {
        a if a < 11.25 => "New Moon",
        a if a < 78.75 => "Waxing Crescent",
        a if a < 101.25 => "First Quarter",
        a if a < 168.75 => "Waxing Gibbous",
        a if a < 191.25 => "Full Moon",
        a if a < 258.75 => "Waning Gibbous",
        a if a < 281.25 => "Last Quarter",
        a if a < 348.75 => "Waning Crescent",
        _ => "New Moon",
    }
}

fn get_moon_phase() -> &'static str {
    return phase_name(get_phase_angle());
}


/// Create the "Right Now" weather table
fn create_right_now_table(forecast: &OpenMeteoForecast) -> Table<'_> {
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
                    .padding(Padding::new(1,1,2,1))
                    // .padding(Padding::uniform(1))
                    .title(Line::from(" Right Now ").light_blue().centered().bold())
            );
}


/// Renders the scatterplot to show the temperature over the next two weeks
fn render_fortnight_scatterplot(
    frame: &mut Frame,
    area: Rect,
    hourly: &Vec<OpenMeteoHourly>,
    daily: &Vec<OpenMeteoPeriod>,
    temp_unit: String,
) {
    const DATA_LENGTH: usize = 336;

    let mut fortnight_hourly: [(f64, f64); DATA_LENGTH] = [(0., 0.); DATA_LENGTH];
    let mut count: usize = 0;
    for i in hourly {
        let mut temp_clone = i.temperature.clone();
        temp_clone.pop();
        let temp_as_float = temp_clone.parse::<f64>().unwrap();
        // Scale the x-coordinate to fit within 0..336 for 14 days of hourly data
        let x_position = count as f64;
        fortnight_hourly[count] = (x_position, temp_as_float);
        count += 1;
    }

    let days: Vec<String> = daily
        .iter()
        .map(|l| {
            let parts: Vec<&str> = l.date.split('-').collect();
            if parts.len() == 3 {
                format!("{}-{}", parts[1], parts[2]) // MM-DD
            } else {
                l.date.clone() // fallback in case the format is unexpected
            }
        })
        .collect();

    let x_labels: Vec<Line> = (0..14) // 14 days in total
        .map(|i| {
            let day = &days[i];
            Line::from(day.as_str())
        })
        .collect();

    let temps: Vec<f64> = fortnight_hourly.iter().map(|(_, temp)| *temp).collect();
    let min_temp = temps.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_temp = temps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let y_min = (min_temp - 5.0).floor();
    let y_max = (max_temp + 5.0).ceil();

    let step = (y_max - y_min) / 4.0;
    let y_labels = (0..5)
        .map(|i| format!("{:.0}", y_min + i as f64 * step))
        .collect::<Vec<_>>();

    let dataset = Dataset::default()
        .marker(Marker::Dot)
        .graph_type(GraphType::Scatter)
        .style(Style::new().yellow())
        .data(&fortnight_hourly);

    let chart = Chart::new(vec![dataset])
        .block(Block::bordered().title(Line::from(" Fortnight Temps ").cyan().centered().bold()))
        .y_axis(
            Axis::default()
                .title(format!("Temp (\u{00B0}{})", temp_unit))
                .bounds([y_min, y_max])
                .style(Style::default().fg(Color::Gray))
                .labels(y_labels),
        )
        .x_axis(
            Axis::default()
                .title("Days")
                .bounds([0., DATA_LENGTH as f64])
                .style(Style::default().fg(Color::Gray))
                .labels(x_labels),
        )
        .hidden_legend_constraints((Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)));

    frame.render_widget(chart, area);
}

/// Renders the scatterplot to show the temperature for the rest of the current day
fn render_temperature_scatterplot(frame: &mut Frame, area: Rect, hourly: &Vec<OpenMeteoHourly>, temp_unit: String) {
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

    
    let temps: Vec<f64> = today_hourly.iter().map(|(_, temp)| *temp).collect();
    let min_temp = temps.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_temp = temps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let y_min = (min_temp - 5.0).floor();
    let y_max = (max_temp + 5.0).ceil();

    let step = (y_max - y_min) / 4.0;
    let y_labels = (0..5)
        .map(|i| format!("{:.0}", y_min + i as f64 * step))
        .collect::<Vec<_>>();

    let dataset = Dataset::default()
            .marker(Marker::Dot)
            .graph_type(GraphType::Scatter)
            .style(Style::new().yellow())
            .data(&today_hourly);
    
    let chart = Chart::new(vec![dataset])
        .block(Block::bordered().title(Line::from(" Today's Temps ").cyan().centered().bold()))
        .y_axis(
            Axis::default()
                .title(format!("Temp (\u{00B0}{})", temp_unit))
                .bounds([y_min, y_max])
                .style(Style::default().fg(Color::Gray))
                .labels(y_labels),
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
fn create_weather_card(period: &OpenMeteoPeriod) -> Table<'_> {
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
    todays_weather_description: Option<String>,
    moon_phase_art: String,
    exit: bool,
    legacy_compliant: bool,
    legacy_mode_active: bool,
    temp_unit: String,
    is_legacy_default: bool,
    show_legacy_popup: bool,
    show_configuration: bool,
}

/// Main Ratatui app for Raijin
impl App {
    /// Runs the application's main loop until the user quits
    fn run(&mut self, terminal: &mut DefaultTerminal, forecast: OpenMeteoForecast, today: Option<String>, moon_phase_art: String) -> io::Result<()> {
        self.temp_unit = env::var("TEMPERATURE_UNIT").unwrap();
        self.is_legacy_default = env::var("DEFAULT_LEGACY").unwrap() == "true";
        self.open_meteo_forecast = forecast;
        self.show_configuration = false;

        self.legacy_mode_active = self.is_legacy_default;
        self.legacy_compliant = today.is_some();
        if self.legacy_compliant {
            self.todays_weather_description = today;
        }


        self.moon_phase_art = moon_phase_art;
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn render_legacy_screen(&self, frame: &mut Frame) {
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

        // Render the current moon phase for tonight
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
            Paragraph::new(self.todays_weather_description.clone().unwrap()).wrap(Wrap { trim: true }).alignment(Alignment::Center) .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(Line::from(" Right Now Details ").light_green().centered().bold())
                        .padding(Padding::uniform(1))
                )
                , description);

        // Render forecast summary details for right now
        frame.render_widget(create_right_now_table(&self.open_meteo_forecast), quick_stats);
        render_temperature_scatterplot(frame, today, &self.open_meteo_forecast.hourly, self.temp_unit.clone());
        
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

    fn render_modern_screen(&self, frame: &mut Frame) {
        use Constraint::{Percentage, Ratio};

        let vertical = Layout::vertical([Percentage(70), Percentage(30)]);
        let [today_area, forecast_area] = vertical.areas(frame.area());
        
        let horizontal = Layout::horizontal([Ratio(2,3), Ratio(1,3)]);
        let [top_left, top_right] = horizontal.areas(today_area);

        let top = Layout::vertical([Percentage(40), Percentage(60)]);
        let [today_info, fortnight_graph] = top.areas(top_left);

        let top2 = Layout::vertical([Ratio(3,4), Ratio(1,4)]);
        let [today, logo_area] = top2.areas(top_right);

        let topest = Layout::horizontal([Ratio(1,2), Ratio(1,2)]);
        let [quick_stats, mid_top] = topest.areas(today_info);
 
        let outer_block = Block::bordered().title(Line::from(" 4-cast ").light_magenta().centered().bold()).padding(Padding::new(0,0,1,0));
        let inner_block = Block::bordered();
        let inner_area = outer_block.inner(forecast_area);

        let upcoming_weather = Layout::horizontal([Ratio(1,4), Ratio(1,4), Ratio(1,4), Ratio(1,4)]);
        let [slot1, slot2, slot3, slot4] = upcoming_weather.areas(inner_area);
        
        frame.render_widget(outer_block, forecast_area);
        frame.render_widget(inner_block, inner_area);

        frame.render_widget(Block::bordered(), mid_top);
        frame.render_widget(Block::new(), fortnight_graph);

        // Render the current moon phase for tonight
        frame.render_widget(
            Paragraph::new(self.moon_phase_art.clone()).alignment(Alignment::Center)
                .block(
                    Block::new()
                        .title(Line::from(" Tonight's Moon Phase ").light_yellow().centered().bold())
                )
                , mid_top);

        // Render the logo on the right-hand side
        let logo = include_str!("./logo.txt");
        frame.render_widget(
            Paragraph::new(logo)
                .alignment(Alignment::Center)
                .style(Style::new().red()),
            logo_area);

        // Render the fortnight weather scatterplot
        render_fortnight_scatterplot(
            frame,
            fortnight_graph,
            &self.open_meteo_forecast.hourly,
            &self.open_meteo_forecast.periods,
            self.temp_unit.clone(),
        );


        // Render forecast summary details for right now
        frame.render_widget(create_right_now_table(&self.open_meteo_forecast), quick_stats);

        // Render the scatterplot for today's temperature
        render_temperature_scatterplot(frame, today, &self.open_meteo_forecast.hourly, self.temp_unit.clone());
        
        // Populate the 4-cast
        for i in 1..5 {
            let mut render_area: Rect = slot1;
            if i == 2 {
                render_area = slot2;
            } else if i == 3 {
                render_area = slot3;
            } else if i == 4 {
                render_area = slot4;
            }

            frame.render_widget(
                create_weather_card(&self.open_meteo_forecast.periods[i]),
                render_area,
            );
        }

        // Show the legacy popup informing the user that they need to configure ZONE and STATE in order to use legacy mode
        if self.show_legacy_popup {
            let area = frame.area().centered(
                Constraint::Percentage(25),
                Constraint::Length(9), // top and bottom border + content
            );
            let title = Line::from("SETUP REQUIRED").light_yellow().centered().bold();
            let content = "\nTo use Legacy Mode, please configure your\nweather ZONE and STATE code.\n\n\nPress Esc to close\nPress C to configure";
            let popup = Paragraph::new(content).block(Block::bordered().title(title));
            frame.render_widget(Clear, area);
            frame.render_widget(popup, area);
        }
    }

    fn render_configuration_screen(&self, frame: &mut Frame) {
        // Show the configuration page
        if self.show_configuration {
            let area = frame.area().centered(
                Constraint::Percentage(50),
                Constraint::Percentage(50), // top and bottom border + content
            );
            let title = Line::from("SETUP REQUIRED").light_yellow().centered().bold();
            let content = "\nTo use Legacy Mode, please configure your\nweather ZONE and STATE code.\n\n\nPress Esc to close\nPress C to configure";
            let popup = Paragraph::new(content).block(Block::bordered().title(title));
            frame.render_widget(Clear, frame.area());
            frame.render_widget(popup, area);
        }
    }

    fn draw(&self, frame: &mut Frame) {
        if self.legacy_mode_active && self.legacy_compliant {
            self.render_legacy_screen(frame);
            return;
        }
        self.render_modern_screen(frame);
        self.render_configuration_screen(frame);
    }

    /// Updates the application's state based on user input
    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                if !self.show_configuration {
                    self.handle_key_event(key_event)
                }
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            KeyCode::Char('l') => self.toggle_legacy_mode(),
            KeyCode::Char('c') => self.configure_app(),
            KeyCode::Esc => {self.show_legacy_popup = false},
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn toggle_legacy_mode(&mut self) {
        if !self.legacy_compliant {
            self.show_legacy_popup = true;
            return;
        }
        self.legacy_mode_active = !self.legacy_mode_active;
    }

    fn configure_app(&mut self) {
        self.show_configuration = true;
        return;
    }
}



/// Get the forecast for the next 7 days as well as today's weather conditions
/// Using this API: <https://api.open-meteo.com/v1/forecast>
fn get_open_meteo_weather(agent: &Agent, weather_codes: serde_json::Value) -> Result<OpenMeteoForecast, ureq::Error> {
    let latitude = env::var("LATITUDE").unwrap();
    let longitude = env::var("LONGITUDE").unwrap();
    let mut temp_unit = env::var("TEMPERATURE_UNIT").unwrap();
    if temp_unit == "F" {
        temp_unit = "fahrenheit".to_string();
    } else {
        temp_unit = "celsius".to_string();
    }
    let mut timezone = env::var("TIMEZONE").unwrap().to_string();
    timezone = encode(&timezone).to_string();

    let url = format!("https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&daily=temperature_2m_max,temperature_2m_min,apparent_temperature_max,apparent_temperature_min,weather_code,precipitation_probability_mean&hourly=temperature_2m,weather_code&current=temperature_2m,apparent_temperature,weather_code&timezone={}&forecast_days=14&temperature_unit={}", latitude.to_string(), longitude.to_string(), timezone, temp_unit);

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
/// Used exclusively for the Right Now Details in Legacy mode
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


/// Create a config directory if it doesn't exist and prepopulate the .env file
fn configure() -> Result<(), Box<dyn std::error::Error>> {
    let folder: PathBuf = dirs::config_dir()
        .expect("configure - Could not find config directory")
        .join("Raijin");

    let file = folder.join(".env");

    if !folder.exists() {
        fs::create_dir(folder)?;
    }

    if !file.exists() {
        fs::write(&file, "ZONE=\"\"\nSTATE=\"\"\nLATITUDE=\"35.9626444\"\nLONGITUDE=\"-83.9167239\"\nTIMEZONE=\"America/New_York\"\nTEMPERATURE_UNIT=\"F\"\nDEFAULT_LEGACY=\"false\"\n")?;
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
    let original_temp_unit = env::var("TEMPERATURE_UNIT").unwrap().to_string();
    let original_default_legacy = env::var("DEFAULT_LEGACY").unwrap().to_string();

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

    if !&params.temp_unit.is_some() {
        new_params.insert("TEMPERATURE_UNIT", original_temp_unit);
    } else {
        new_params.insert("TEMPERATURE_UNIT", params.temp_unit.clone().unwrap().to_string());
    }

    if !&params.default_legacy.is_some() {
        new_params.insert("DEFAULT_LEGACY", original_default_legacy);
    } else {
        new_params.insert("DEFAULT_LEGACY", params.default_legacy.clone().unwrap().to_string());
    }

    let file: PathBuf = dirs::config_dir()
        .expect("update_config - Could not find config directory")
        .join("Raijin")
        .join(".env");

    let file_data = format!(
        "ZONE=\"{}\"\nSTATE=\"{}\"\nLATITUDE=\"{}\"\nLONGITUDE=\"{}\"\nTIMEZONE=\"{}\"\nTEMPERATURE_UNIT=\"{}\"\nDEFAULT_LEGACY=\"{}\"",
        new_params["ZONE"],
        new_params["STATE"],
        new_params["LAT"],
        new_params["LONG"],
        new_params["TIMEZONE"],
        new_params["TEMPERATURE_UNIT"],
        new_params["DEFAULT_LEGACY"],
    );
    fs::write(&file, file_data)?;

    Ok(())
}


fn check_legacy_compliance() -> bool {
    let state = env::var("STATE").unwrap().to_string();
    let zone = env::var("ZONE").unwrap().to_string();

    return zone != "" && state != "";
}



fn main() -> io::Result<()> {
    let _ = configure();
    let file = dirs::config_dir().expect("main - Could not find config directory").join("Raijin").join(".env");
    let _ = dotenv::from_path(&file).expect("main - Could not find .env file");

    let args = Args::parse();

    match &args.command {
        Some(Commands::Edit { timezone, lat, long, state, zone, temp_unit, default_legacy }) => {
            let config_params = ConfigParams {
                timezone: timezone.clone(),
                lat: lat.clone(),
                long: long.clone(),
                state: state.clone(),
                zone: zone.clone(),
                temp_unit: temp_unit.clone(),
                default_legacy: default_legacy.clone(),
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

    let is_legacy_compliant: bool = check_legacy_compliance();

    // If the legacy variables aren't set, don't setup the "Right Now Details"
    let mut today = None;
    if is_legacy_compliant {
        let now = Instant::now();
        let nws_periods = get_nws_weather_periods(&agent).unwrap();
        println!("nws_periods: {:.2?}", now.elapsed());
        today = Some(nws_periods[0].detailed_forecast.clone());
    }

    // Get all the weather data
    let now = Instant::now();
    let open_meteo_forecast = get_open_meteo_weather(&agent, weather_codes).unwrap();
    println!("openmeteo: {:.2?}", now.elapsed());

    // Get the moon phase and use that to get the right art file
    let phase_file = MOON_PHASE_ART_DIR.get_file(format!("{}.txt", get_moon_phase())).unwrap();
    let moon_phase_art = phase_file.contents_utf8().unwrap();
    
    // Initialize the TUI
    let mut terminal = ratatui::init();
    let app_result = App::default().run(&mut terminal, open_meteo_forecast, today, moon_phase_art.to_string());
    // Restore the terminal before we leave
    ratatui::restore();
    app_result
}
