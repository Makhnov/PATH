pub type Error = &'static str;
pub type Result<T> = core::result::Result<T, Error>;

// La fin du fichier renvoie une erreur EOF, elle n'est considérée comme telle que lorsqu'on à un bloc non complet.
// Dans le cas contraire elle est ignorée. Cf. gps.rs : Err(EOF) => return Ok(())
pub const EOF: Error = "unexpected end of GPS data";

// TODO Les erreurs sont spécifiées dans un champ texte, on devrait au moins définir les grands types (formatage, option, construction du graphe, etc.)
