#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{egui::Visuals};
use egui::{Color32, RichText};
//use egui::ahash::HashMap;
use std::{
	collections::HashMap,
	fs::{self, File, OpenOptions},
	io::{BufRead, BufReader, Write},
	path::Path,
	sync::{Arc, Mutex},
	thread,
	time::{Duration, SystemTime}
};
use rodio::{OutputStream, Sink, Source};
use rand::Rng;

// This is a really stupid dependency but as it turns out I guess this is a non-trivial problem???
// Rodio's built in functionality for this just doesn't work most of the time for some reason.
use mp3_duration;

fn main() -> Result<(), eframe::Error> {
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default().with_inner_size([690.0, 360.0]),
		..Default::default()
	};
	let app = App::default();
	let mut sink = app.sink.clone();
	let mut shared_data = app.appdata.clone();
	thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            if sink.lock().unwrap().empty() {
				let sel_type = shared_data.lock().unwrap().sel_type.clone();
				let _ = handle_song_end(sel_type, &mut shared_data, &mut sink);
			}
        }
	});
	eframe::run_native(
		"Dreamer",
		options,
		Box::new(|_cc| {Box::new(app)}),
	)
}

//static mut DATA: Vec<(String, String)> = Vec::new();

struct SongInfo {
	name: String,
	artist: String,
	genre: String,
	nodisplay_time_listened: u128,
}

impl Default for SongInfo {
	fn default() -> Self {
		Self {
			name: format!(""),
			artist: format!(""),
			genre: format!(""),
			nodisplay_time_listened: 0,
		}
	}
}

#[derive(PartialEq)]
#[derive(Debug)]
#[derive(Clone)]
enum SelectionType {None,Loop,Random,Next}

// This is everything that needs to be shared across threads
struct SharedAppData {
	sel_type: SelectionType,
	cur_song_index: usize,
	songs_list: Vec<String>,
	start_system: SystemTime,
	start_milis: u64,
	position: u64,
	total_duration: u64,
	current_song_info: SongInfo,
	dat_map: HashMap<String, String>,
	song_data_exists: bool,
}

impl Default for SharedAppData {
	fn default() -> Self {
		let mut songls: Vec<String> = vec![];
		let paths = fs::read_dir("songs\\");
		let mut data_map: HashMap<String,String> = HashMap::new();

		initialize_data_map(&mut data_map);

		match paths {
			Ok(pat) => {
				songls.clear();
				for p in pat {
					if let Ok(a) = p {
						songls.push(a.file_name().into_string().unwrap());
					}
				}
			},
			Err(_) => {
				songls.clear();
				songls.push(format!("Error in fetching Music directory"));
			},
		}

		let item = songls.get(0).unwrap();
		let map_data = data_map.get(item);

		let mut new_si = SongInfo::default();

		let data_found = if let Some(map_data) = map_data {
			let collection = map_data.split(',').collect::<Vec<&str>>();

			new_si.name = (**collection.get(1).unwrap()).to_string();
			new_si.artist = (**collection.get(2).unwrap()).to_string();
			new_si.genre = (**collection.get(3).unwrap()).to_string();
			new_si.nodisplay_time_listened = (**collection.get(4).unwrap()).to_string().parse().unwrap();
			true
		} else {false};

		Self {
			sel_type: SelectionType::None,
			cur_song_index: 0,
			songs_list: songls,
			start_system: SystemTime::now(),
			total_duration: 0,
			start_milis: 0,
			position: 0,
			current_song_info: new_si,
			dat_map: data_map,
			song_data_exists: data_found,
		}
	}
}

struct App {
	sink: Arc<Mutex<rodio::Sink>>,
	appdata: Arc<Mutex<SharedAppData>>,
	
	// Not accessed from other threads ***************
	search_text: String,
	error: String,
	volume: f32,

	_stream: OutputStream, // THIS HAS TO EXIST otherwise the lifetime causes the program to crash
}

impl Default for App {
	fn default() -> Self {
		let (i1, i2) = OutputStream::try_default().unwrap();
		
		Self {
			_stream: i1,
			sink: Arc::new(Mutex::new(rodio::Sink::try_new(&i2).unwrap())),
			appdata: Arc::new(Mutex::new(SharedAppData::default())),
			search_text: format!(""),
			error: format!(""),
			volume: 0.5,
		}
	}
}

impl eframe::App for App {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		ctx.request_repaint_after(Duration::from_millis(250));
		ctx.set_visuals(Visuals::dark());
		ctx.set_pixels_per_point(1.33);

		egui::CentralPanel::default().show(ctx, |ui| {
			ui.heading("Kate's Untitled MP3 Player");
			ui.horizontal(|ui| {
				let mut appdata = self.appdata.lock().unwrap();
				ui.label("When a song ends: ");
				egui::ComboBox::from_label("")
					.selected_text(format!("{:?}", appdata.sel_type))
					.show_ui(ui, |ui| {
						ui.selectable_value(&mut appdata.sel_type, SelectionType::None, "None");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Loop, "Loop");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Random, "Random");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Next, "Next");
					}
				);
				//ui.checkbox(&mut self.loopy, "Loop songs on finish");
			});
			ui.horizontal(|ui| {
				if ui.button("Refresh").clicked() {
					let mut appdata = self.appdata.lock().unwrap();
					appdata.songs_list.clear();

					let paths = fs::read_dir("songs\\");
					match paths {
						Ok(pat) => {
							for p in pat {
								if let Ok(a) = p {
									appdata.songs_list.push(a.file_name().into_string().unwrap());
								}
							}
						},
						Err(_) => {
							appdata.songs_list.push(format!("Error in fetching Music directory"));
						},
					}
				}
				ui.add(egui::TextEdit::singleline(&mut self.search_text).hint_text("Search..."));
			});
			ui.add_space(10.0);
			ui.horizontal(|ui| {
				ui.set_min_height(200.0);
				ui.vertical(|ui| {
					egui::ScrollArea::vertical().show(ui, |ui| {
						ui.set_max_width(275.0);
						ui.set_min_width(275.0);
						let mut song_change_triggered = false;
						let mut activate_song = 0;
						let current_song_index_clone = self.appdata.lock().unwrap().cur_song_index;
						{
							let mut aplock = self.appdata.lock().unwrap();
							for (index, dir) in (&mut aplock.songs_list).into_iter().enumerate() {
								if self.search_text.len() == 0 || 
									dir.to_ascii_lowercase().contains(&self.search_text.to_ascii_lowercase())
								{
									let mut clicked = false;
									ui.horizontal(|ui| {
										if current_song_index_clone == index {
											ui.label(RichText::new(dir.clone()).underline().strong());
										}
										else {ui.label(dir.clone());}
										if ui.button("▶").clicked() {
											song_change_triggered = true;
											clicked = true;
											activate_song = index;
										}
									});
								}
							}
						}
						if song_change_triggered {
							let res = {
								let mut appdata = self.appdata.lock().unwrap();
								let mut item = appdata.songs_list.get(activate_song).unwrap().clone();
								appdata.cur_song_index = activate_song;
								
								let data_exists  = update_cursong_data(&mut appdata, &mut item);
								let fp = format!("songs\\{}", item);
								let file = File::open(&fp).unwrap();

			
								appdata.start_system = SystemTime::now();
								let reader = BufReader::new(file);
								appdata.song_data_exists = data_exists;
								(reader, fp)
							};
		
							self.error = play_song(&mut self.appdata, &mut self.sink, res.0, &res.1);
						}
					});
				});
				
				ui.vertical(|ui| {
					let mut appdata = self.appdata.lock().unwrap();
					ui.set_max_width(200.0);
					ui.vertical_centered(|ui| {
						ui.heading("Song Info");
					});
					ui.horizontal(|ui| {
						let song_label = ui.label("Song");
						ui.text_edit_singleline(&mut appdata.current_song_info.name).labelled_by(song_label.id);
					});
					ui.horizontal(|ui| {
						let artist_label = ui.label("Artist");
						ui.text_edit_singleline(&mut appdata.current_song_info.artist).labelled_by(artist_label.id);
					});
					ui.horizontal(|ui| {
						let genre_label = ui.label("Genre");
						ui.text_edit_singleline(&mut appdata.current_song_info.genre).labelled_by(genre_label.id);
					});
					if ui.button("Save").clicked() {
						let sindex = appdata.cur_song_index;
						save_data(
							&mut appdata, sindex
						);
						appdata.song_data_exists = true;
					}
					if !appdata.song_data_exists {
						ui.horizontal( |ui| {
							ui.label(RichText::new("Warning:").color(Color32::YELLOW));
							ui.label("No associated saved data found");
						});
					}
				});
			});
		});

		egui::TopBottomPanel::bottom("Player").show(ctx, |ui| {
			ui.horizontal(|ui| {
				let appdata = self.appdata.lock().unwrap();
				ui.label(format!("Currently Playing: {}", appdata.songs_list.get(appdata.cur_song_index as usize).unwrap()));
				
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
					ui.label(&self.error)
				});
				
			});
			ui.horizontal(|ui| {
				if ui.button("Play").clicked() {
					let fp = {
						let a_lock = self.appdata.lock().unwrap();
						format!("songs\\{}", a_lock.songs_list.get(a_lock.cur_song_index).unwrap()
					)};
					let open_file = File::open(&fp);

					if let Ok(open_file) = open_file {
						let reader = BufReader::new(open_file);
						
						self.error = play_song(&mut self.appdata, &mut self.sink, reader, &fp);
					}
					else {
						self.error = format!("File not found: {}", &fp);
					}
				}
				{
					let sink = self.sink.lock().unwrap();
					match sink.is_paused() {
						true => if ui.button("Unpause").clicked() {
							sink.play();
							self.appdata.lock().unwrap().start_system = SystemTime::now()
						},
						false => if ui.button("Pause").clicked() {
							sink.pause();
							let mut appdata = self.appdata.lock().unwrap();
							appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
							let sindex = appdata.cur_song_index;
							save_data_noinsert(
								&mut appdata, sindex
							);
							appdata.start_milis = appdata.position;
						},
					}
				}
				
				if ui.button("Stop").clicked() {
					let mut appdata = self.appdata.lock().unwrap();
					appdata.position = 0;
					appdata.start_system = SystemTime::now();
					appdata.start_milis = 0;
					self.sink.lock().unwrap().skip_one();
				}
				
				let og_spacing = ui.spacing().slider_width;
				let size = ctx.available_rect().size().x - 360.0;
				ui.spacing_mut().slider_width = size;

				let dragged = {
					let mut slappdata = self.appdata.lock().unwrap();

					let secs = slappdata.position / 1000;
					let max_duration = slappdata.total_duration;
					
					let seeker = ui.add(
						egui::Slider::new(&mut slappdata.position, 0..=max_duration)
						.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 1.0 })
						.show_value(false)
						.text(format!("{}:{}{}", secs / 60, if secs % 60 < 10 {"0"} else {""}, secs % 60))
					);
					ui.spacing_mut().slider_width = og_spacing;
					seeker.dragged()
				};

				// This is to prevent an issue that would cause an infinite loop somehow
				if dragged {
					let mut appdata = self.appdata.lock().unwrap();
					let _ = self.sink.lock().unwrap().try_seek(Duration::from_millis(appdata.position));
					appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
					appdata.start_system = SystemTime::now();
					appdata.start_milis = appdata.position;
				} else {
					let empt = self.sink.lock().unwrap().empty();
					if empt {
						let sel_type = self.appdata.lock().unwrap().sel_type.clone();
						self.error = handle_song_end(sel_type, &mut self.appdata, &mut self.sink);
					}
				}
				let sink = self.sink.lock().unwrap();
				let mut appdata = self.appdata.lock().unwrap();
				if appdata.position < appdata.total_duration && !sink.is_paused() && !sink.empty() {
					appdata.position = appdata.start_system.elapsed().unwrap().as_millis() as u64 + appdata.start_milis;
				}
				
				ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
					ui.add( egui::Slider::new(&mut self.volume, -0.2..=1.0).show_value(false).text("Volume")).on_hover_text_at_pointer(format!("{}", self.volume));

					let falloff = if self.volume <= -0.195 {0.0} else {(self.volume * 6.908).exp() / 1000.0};
					if sink.volume() != falloff {
						sink.set_volume(falloff);
					}
				});
			});
		});
	}
}

fn play_song(appdata: &mut Arc<Mutex<SharedAppData>>, sink: &mut Arc<Mutex<Sink>>, reader: BufReader<File>, fp: &str) -> String {
	let elem = rodio::Decoder::new_mp3(reader);
	let sink = sink.lock().unwrap();
	match elem {
		Ok(a) => {
			let path = Path::new(&fp);
			let path_test = mp3_duration::from_path(&path);
			if let Ok(path_test) = path_test {
				appdata.lock().unwrap().total_duration = path_test.as_millis() as u64;
			} else {
				let t = a.total_duration();
				if let Some(t) = t {
					appdata.lock().unwrap().total_duration = t.as_millis() as u64;
				} else {
					return format!("Error - Couldn't determine song length");
				}
			}
			appdata.lock().unwrap().total_duration = mp3_duration::from_path(&path).unwrap().as_millis() as u64;
			sink.stop();

			if !sink.is_paused() && !sink.empty() {
				let mut aplock = appdata.lock().unwrap();
				aplock.current_song_info.nodisplay_time_listened += aplock.start_system.elapsed().unwrap().as_millis();
				let sindex = aplock.cur_song_index;
				save_data_noinsert(
					&mut aplock, sindex
				);
			}
			let mut appdata_mut = appdata.lock().unwrap();
			appdata_mut.start_system = SystemTime::now();
			appdata_mut.position = 0;
			appdata_mut.start_milis = 0;

			sink.append(a); 
			format!("")},
		Err(_) => format!("Error in decoding song :("),
	}
}

fn save_data_noinsert(app: &mut SharedAppData, cur_song_index: usize) {
	let current_song_info = &app.current_song_info;
	let dat_map = &mut app.dat_map;
	let songs_list = &app.songs_list;
	let current_s = songs_list.get(cur_song_index).unwrap();
	let data = format!("{},{},{},{},{}", current_s, current_song_info.name, current_song_info.artist, current_song_info.genre, current_song_info.nodisplay_time_listened);
	
	if dat_map.contains_key(current_s) {
		dat_map.insert(current_s.clone(), data);
	} else {
		return;
	}
	fs::write("data.csv", "").expect("Unable to write file");

	for keys in dat_map.keys() {
		let mut f = OpenOptions::new()
			.append(true)
			.open("data.csv")
			.unwrap();
		let _ = writeln!(f, "{}", dat_map.get(keys).unwrap()).is_ok();
	}
}

fn save_data(app: &mut SharedAppData, cur_song_index: usize) {
	let current_song_info = &app.current_song_info;
	let dat_map = &mut app.dat_map;
	let songs_list = &app.songs_list;
	let current_s = songs_list.get(cur_song_index).unwrap();
	let data = format!("{},{},{},{},{}", current_s, current_song_info.name, current_song_info.artist, current_song_info.genre, current_song_info.nodisplay_time_listened);
	
	dat_map.insert(current_s.clone(), data);
	fs::write("data.csv", "").expect("Unable to write file");

	for keys in dat_map.keys() {
		let mut f = OpenOptions::new()
			.append(true)
			.open("data.csv")
			.unwrap();
		let _ = writeln!(f, "{}", dat_map.get(keys).unwrap()).is_ok();
	}
}

fn initialize_data_map(data_map: &mut HashMap<String,String>) {
	let fp = format!("data.csv");
	let file = File::open(&fp);

	if let Ok(file) = file {
		let reader = BufReader::new(file);
		for line in reader.lines() {
			let unwrapped_line = line.unwrap();
			let unw_clone = unwrapped_line.clone();
			let collection = unwrapped_line.split(',').collect::<Vec<&str>>();
	
			let key = (**collection.get(0).unwrap()).to_string();

			data_map.insert(key, unw_clone);
		}
	}
}

fn update_cursong_data(appdata: &mut SharedAppData, song_name: &str) -> bool {
	let map_data = appdata.dat_map.get(song_name);

	if let Some(map_data) = map_data {
		let collection = map_data.split(',').collect::<Vec<&str>>();

		appdata.current_song_info.name = (**collection.get(1).unwrap()).to_string();
		appdata.current_song_info.artist = (**collection.get(2).unwrap()).to_string();
		appdata.current_song_info.genre = (**collection.get(3).unwrap()).to_string();
		appdata.current_song_info.nodisplay_time_listened = (**collection.get(4).unwrap()).to_string().parse().unwrap();
		return true;
	} else {
		appdata.current_song_info.name   = format!("");
		appdata.current_song_info.nodisplay_time_listened = 0;
		return false;
	}
}

fn handle_song_end(sel_type: SelectionType, app: &mut Arc<Mutex<SharedAppData>>, sink: &mut Arc<Mutex<Sink>>) -> String {
	return match sel_type {
			SelectionType::None => {format!("")},
			SelectionType::Loop => {
			let fp = {
				let ada = app.lock().unwrap();
				format!("songs\\{}", ada.songs_list.get(ada.cur_song_index).unwrap())
			};
			let open_file = File::open(&fp);
			if let Ok(open_file) = open_file {
				let reader = {
					let mut appdata = app.lock().unwrap();
					let reader = BufReader::new(open_file);
					
					appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
					let sindex = appdata.cur_song_index;
					save_data_noinsert(
						&mut appdata, sindex
					);
					reader
				};
				
				play_song(app, sink, reader, &fp)
			}
			else {
				format!("File not found: {}", &fp)
			}
		},
		SelectionType::Next => {
			let fp = {
				let mut appdata = app.lock().unwrap();
				appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
				appdata.start_system = SystemTime::now();
				let sindex = appdata.cur_song_index;
				save_data_noinsert(
					&mut appdata, sindex
				);
				
				appdata.cur_song_index = if appdata.cur_song_index + 1 >= appdata.songs_list.len() {0} else {appdata.cur_song_index + 1};
				
				let mut item = appdata.songs_list.get(appdata.cur_song_index).unwrap().clone();
				let fp = format!("songs\\{}", item);
				appdata.song_data_exists = update_cursong_data(&mut appdata, &mut item);
				fp
			};
			let file = File::open(&fp).unwrap();
			let reader = BufReader::new(file);
			
			play_song(app, sink, reader, &fp)
		},
		SelectionType::Random => {
			let fp = {
				let mut appdata = app.lock().unwrap();
				appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
				appdata.start_system = SystemTime::now();
				let sindex = appdata.cur_song_index;
				save_data_noinsert(
					&mut appdata, sindex
				);
				
				appdata.cur_song_index = rand::thread_rng().gen_range(0..appdata.songs_list.len());
				
				let mut item = appdata.songs_list.get(appdata.cur_song_index).unwrap().clone();
				let fp = format!("songs\\{}", item);
				appdata.song_data_exists = update_cursong_data(&mut appdata, &mut item);
				fp
			};
			let file = File::open(&fp).unwrap();
			let reader = BufReader::new(file);
			
			play_song(app, sink, reader, &fp)
		},
	};
}