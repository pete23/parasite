# Parasite: Vocal Sample Pack Creator

Parasite is a TUI (Terminal User Interface) application that helps you create vocal sample packs from VTT (WebVTT) files and corresponding WAV audio files.

WARNING! This was 100pc vibe coded with Claude Code. It "works" on my machine, keyboard layout, and has successfully allowed me to create a little sample pack from a bunch of whisper'd wavs. YMMV and if it eats your dog whilst bootstrapping AGI on your toaster... Caveat clonor, please reread sections 5 and 6 of the license:-)

## Features

- Search for text within VTT files using incremental search
- Browse search results with timestamps
- Extract audio samples based on selected text
- Save samples to the output directory
- Add context lines above and below search results
- Adjust timestamp boundaries for precise extraction
- Preview audio before extracting

## Prerequisites

- Rust toolchain (cargo, rustc)
- ffmpeg (used for extracting audio segments)

## Installation

1. Clone the repository
2. Build the application:
   ```bash
   cargo build --release
   ```
3. Run the application:
   ```bash
   cargo run
   ```

## Command-Line Options

Parasite supports the following command-line options:

```
Usage: parasite [OPTIONS]

Options:
  -i, --input-dir <INPUT_DIR>    Directory containing VTT and WAV files [default: data]
  -o, --output-dir <OUTPUT_DIR>  Directory for saving extracted samples [default: output]
  -h, --help                     Print help
  -V, --version                  Print version
```

Example:
```bash
cargo run -- --input-dir my_transcripts --output-dir my_samples
```

## Usage

1. Type your search query directly (matches all words in any order)
2. Use Up/Down arrow keys to navigate search results
3. Use `+`/`-` to adjust context lines around matches
4. Preview selections with `Tab` before extracting
5. Adjust timing with `,`/`.` (start time) and `[`/`]` (end time)
6. Use fine adjust keys (`<`, `>`, `{`, `}`) for precise timing (25ms instead of 100ms)
7. Press `Esc` to reset timing adjustments if needed
8. Press Enter to extract the currently selected sample
9. Extracted samples are saved to the output directory

## Key Bindings

- Type directly to search (no search mode needed)
- Up/Down - Navigate search results
- Enter - Extract the selected sample
- `+`/`-` - Increase/decrease context lines
- `,`/`.` - Adjust start time backward/forward (100ms)
- `<`/`>` - Fine adjust start time (25ms)
- `[`/`]` - Adjust end time backward/forward (100ms)
- `{`/`}` - Fine adjust end time (25ms)
- `Esc` - Reset timestamps to original values
- `Tab` - Preview selected sample
- `q` - Quit application

## Project Structure

- `src/` - Source code
- `docs/` - Documentation files
- `data/` - Default directory for input VTT and WAV files (create this yourself)
- `output/` - Default directory for extracted samples (created automatically)

## License

Eclipse Public License 2.0 (EPL-2.0)
