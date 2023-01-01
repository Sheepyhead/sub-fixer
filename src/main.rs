#![feature(is_some_and)]

use mimalloc::MiMalloc;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

static VIDEO_FILE_EXTENSIONS: [&str; 2] = ["mp4", "mkv"];

fn main() {
    let path = env::args().skip(1).next().expect("Missing path argument");

    let root_contents = fs::read_dir(path).expect("Cannot read directory in given path");

    for entry in root_contents.filter_map(|entry| entry.ok()) {
        if entry.file_type().is_err() || !entry.file_type().unwrap().is_dir() {
            println!("Expected directory, but file type is erroneous or not a directory");
            continue;
        }

        if let Ok(folder_type) =
            get_folder_type(&entry.path()).map_err(|err| println!("{:?}: {:?}", entry.path(), err))
        {
            match folder_type {
                FolderType::Movie(file_name) => process_movie(&entry.path(), &file_name)
                    .map_err(|err| println!("{:?}: {:?}", entry.path(), err))
                    .ok(),
                FolderType::Show(file_names) => process_show(&entry.path(), &file_names)
                    .map_err(|err| println!("{:?}: {:?}", entry.path(), err))
                    .ok(),
                FolderType::ShowWithSeasons => process_show_with_seasons(&entry.path())
                    .map_err(|errs| {
                        errs.iter()
                            .for_each(|err| println!("{:?}: {:?}", entry.path(), err))
                    })
                    .ok(),
            };
        };
    }
}

#[derive(Debug)]
enum ProcessingError {
    NoVideoFilesAndNoSeasonFoldersFound,
    NoSubFolderFoundForMovie,
    NoSubFolderFoundForShow,
    NoSubFileFoundForMovie,
    NoSubFileFoundForShow(String),
    FailedToCopySubFile(String),
    SubFolderForShowWithUnknownName(String),
}

#[derive(Debug)]
enum FolderType {
    Movie(String),
    Show(Vec<String>),
    ShowWithSeasons,
}

fn process_movie(path: &Path, file_name: &String) -> Result<(), ProcessingError> {
    let sub_folder = fs::read_dir(path)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_name().to_str().unwrap().to_lowercase() == "subs"
                && entry.file_type().unwrap().is_dir()
        })
        .next();
    match sub_folder {
        Some(folder) => {
            let subs = fs::read_dir(folder.path())
                .unwrap()
                .filter(|entry| {
                    entry.as_ref().unwrap().file_name().to_str().unwrap() == "2_English.srt"
                })
                .flatten()
                .next();
            match subs {
                Some(subs) => move_subtitle_file(file_name, path, &subs.path()),
                None => Err(ProcessingError::NoSubFileFoundForMovie),
            }
        }
        None => Err(ProcessingError::NoSubFolderFoundForMovie),
    }
}

fn move_subtitle_file(file_name: &String, to: &Path, from: &Path) -> Result<(), ProcessingError> {
    let mut final_file_name = file_name.clone();
    final_file_name.push_str(".srt");
    let mut destination = PathBuf::from(to);
    destination.push(final_file_name);
    match fs::copy(from, destination) {
        Ok(..) => Ok(()),
        Err(err) => Err(ProcessingError::FailedToCopySubFile(err.to_string())),
    }
}

fn process_show(path: &Path, file_names: &Vec<String>) -> Result<(), ProcessingError> {
    let subs_folder = fs::read_dir(path)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().unwrap().is_dir()
                && entry.file_name().to_str().unwrap().to_lowercase() == "subs"
        })
        .next();
    match subs_folder {
        Some(subs_folder) => {
            let subs_folders = fs::read_dir(subs_folder.path())
                .unwrap()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_ok_and(|ftype| ftype.is_dir()))
                .collect::<Vec<_>>();
            for entry in subs_folders.iter() {
                let folder_name = entry.file_name().to_string_lossy().to_string();
                if !file_names.contains(&folder_name) {
                    return Err(ProcessingError::SubFolderForShowWithUnknownName(
                        folder_name,
                    ));
                }
                let sub_file = fs::read_dir(entry.path())
                    .unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        entry.file_type().is_ok_and(|ftype| ftype.is_file())
                            && entry.file_name().to_str().unwrap() == "2_English.srt"
                    })
                    .next();
                match sub_file {
                    Some(file) => match move_subtitle_file(&folder_name, path, &file.path()) {
                        Err(err) => return Err(err),
                        _ => {}
                    },
                    None => {
                        return Err(ProcessingError::NoSubFileFoundForShow(
                            entry.path().to_string_lossy().to_string(),
                        ))
                    }
                }
            }
            Ok(())
        }
        None => Err(ProcessingError::NoSubFolderFoundForShow),
    }
}

fn process_show_with_seasons(path: &Path) -> Result<(), Vec<ProcessingError>> {
    let mut errors = vec![];
    let season_folders = fs::read_dir(path)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    for folder in season_folders.iter() {
        let contents = fs::read_dir(folder).unwrap().filter_map(|entry| entry.ok());
        let video_file_names = contents
            .filter(|entry| {
                entry.file_type().is_ok_and(|ftype| ftype.is_file())
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|ext| VIDEO_FILE_EXTENSIONS.contains(&ext.to_str().unwrap()))
            })
            .map(|entry| remove_file_extension(entry.file_name().to_string_lossy().to_string()))
            .collect::<Vec<_>>();
        if let Err(err) = process_show(&folder, &video_file_names) {
            errors.push(err);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn get_folder_type(path: &Path) -> Result<FolderType, ProcessingError> {
    let contents = fs::read_dir(path)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect::<Vec<_>>();
    let video_files = contents
        .iter()
        .filter(|entry| {
            entry.file_type().is_ok_and(|ftype| ftype.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|ext| VIDEO_FILE_EXTENSIONS.contains(&ext.to_str().unwrap()))
        })
        .collect::<Vec<_>>();
    match video_files.len() {
        0 => {
            // No video files in folder, check instead for season folders and if it has folders not named "subs", assume it's a season show
            if contents
                .iter()
                .filter(|entry| {
                    entry.file_type().unwrap().is_dir()
                        && entry.file_name().to_str().unwrap().to_lowercase() != "subs"
                })
                .count()
                > 0
            {
                Ok(FolderType::ShowWithSeasons)
            } else {
                // Either there are no folders, or the only folder in it is called "subs"
                Err(ProcessingError::NoVideoFilesAndNoSeasonFoldersFound)
            }
        }
        1 => {
            // One video file, assume it's a movie
            Ok(FolderType::Movie(remove_file_extension(
                video_files
                    .iter()
                    .next()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .to_string(),
            )))
        }
        _ => {
            // More than one video file, assume it's a show
            Ok(FolderType::Show(
                video_files
                    .iter()
                    .map(|entry| {
                        remove_file_extension(entry.file_name().to_string_lossy().to_string())
                    })
                    .collect(),
            ))
        }
    }
}

fn remove_file_extension(value: String) -> String {
    let mut chars = value.chars();
    chars.next_back();
    chars.next_back();
    chars.next_back();
    chars.next_back();
    chars.collect()
}
