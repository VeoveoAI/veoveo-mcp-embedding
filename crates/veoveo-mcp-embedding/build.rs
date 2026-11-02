//! Embeds the skills under `skills/`, which the server serves at `skill://embedding/…`.
fn main() {
    veoveo_skills::build::embed_skills("skills").expect("Embedding's skills");
}
