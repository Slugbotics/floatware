# Do not contribute AI-generated code

### Code style guidelines
- Try to make some effort to write comments. I know it's hard. If not for individual statements, at least functions and
modules if you can. Do NOT put LLM-generated comments, if you do I will try my hardest to get you kicked from the
Slugbotics Github org.
- Follow the settings in `rustfmt.toml`
- Imports: one blank line between `use` statements. Order is: `crate`, `std`, `esp_idf_svc`, and then everything else.
- Please keep doc comments to less than 80 characters wide (unless they start with `TODO: `). Normal comments can be
longer.
- Lines should be <100 characters
- Put a line consisting of eighty slashes between major parts of the file (e.g. separating the imports from the rest) to
help break it up visually and make it easier for your eyes to find their way
