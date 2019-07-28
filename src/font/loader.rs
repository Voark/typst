//! Loading of fonts matching queries.

use std::cell::{RefCell, Ref};
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};

use super::{Font, FontInfo, FontClass, FontProvider};


/// Serves fonts matching queries.
pub struct FontLoader<'p> {
    /// The font providers.
    providers: Vec<Box<dyn FontProvider + 'p>>,
    /// The internal state. Uses interior mutability because the loader works behind
    /// an immutable reference to ease usage.
    state: RefCell<FontLoaderState>,
}

/// Internal state of the font loader (seperated to wrap it in a `RefCell`).
struct FontLoaderState {
    /// The loaded fonts alongside their external indices. Some fonts may not
    /// have external indices because they were loaded but did not contain the
    /// required character. However, these are still stored because they may
    /// be needed later. The index is just set to `None` then.
    fonts: Vec<(Option<usize>, Font)>,
    /// Allows to retrieve a font (index) quickly if a query was submitted before.
    query_cache: HashMap<FontQuery, usize>,
    /// Allows to re-retrieve loaded fonts by their info instead of loading them again.
    info_cache: HashMap<FontInfo, usize>,
    /// Indexed by external indices (the ones inside the tuples in the `fonts` vector)
    /// and maps to internal indices (the actual indices into the vector).
    inner_index: Vec<usize>,
}

impl<'p> FontLoader<'p> {
    /// Create a new font loader.
    pub fn new() -> FontLoader<'p> {
        FontLoader {
            providers: vec![],
            state: RefCell::new(FontLoaderState {
                query_cache: HashMap::new(),
                info_cache: HashMap::new(),
                inner_index: vec![],
                fonts: vec![],
            }),
        }
    }

    /// Add a font provider to this loader.
    pub fn add_font_provider<P: FontProvider + 'p>(&mut self, provider: P) {
        self.providers.push(Box::new(provider));
    }

    /// Returns the font (and its index) best matching the query, if there is any.
    pub fn get(&self, query: FontQuery) -> Option<(usize, Ref<Font>)> {
        // Load results from the cache, if we had the exact same query before.
        let state = self.state.borrow();
        if let Some(&index) = state.query_cache.get(&query) {
            // The font must have an external index already because it is in the query cache.
            // It has been served before.
            let extern_index = state.fonts[index].0.unwrap();
            let font = Ref::map(state, |s| &s.fonts[index].1);

            return Some((extern_index, font));
        }
        drop(state);

        // The outermost loop goes over the fallbacks because we want to serve the
        // font that matches the first possible class.
        for class in &query.fallback {
            // For each class now go over all fonts from all font providers.
            for provider in &self.providers {
                for info in provider.available().iter() {
                    let viable = info.classes.contains(class);
                    let matches = viable && query.classes.iter()
                        .all(|class| info.classes.contains(class));

                    if matches {
                        let mut state = self.state.borrow_mut();

                        // Check if we have already loaded this font before, otherwise,
                        // we will load it from the provider.
                        let index = if let Some(&index) = state.info_cache.get(info) {
                            index
                        } else if let Some(mut source) = provider.get(info) {
                            let mut program = Vec::new();
                            source.read_to_end(&mut program).ok()?;
                            let font = Font::new(program).ok()?;

                            // Insert it into the storage and cache it by its info.
                            let index = state.fonts.len();
                            state.info_cache.insert(info.clone(), index);
                            state.fonts.push((None, font));

                            index
                        } else {
                            // Strangely, this provider lied and cannot give us the promised font.
                            continue;
                        };

                        // Proceed if this font has the character we need.
                        let has_char = state.fonts[index].1.mapping.contains_key(&query.character);
                        if has_char {
                            // This font is suitable, thus we cache the query result.
                            state.query_cache.insert(query, index);

                            // Now we have to find out the external index of it or assign
                            // a new one if it has none.
                            let external_index = state.fonts[index].0.unwrap_or_else(|| {
                                // We have to assign an external index before serving.
                                let new_index = state.inner_index.len();
                                state.inner_index.push(index);
                                state.fonts[index].0 =  Some(new_index);
                                new_index
                            });

                            // Release the mutable borrow to be allowed to borrow immutably.
                            drop(state);

                            // Finally, get a reference to the actual font.
                            let font = Ref::map(self.state.borrow(), |s| &s.fonts[index].1);
                            return Some((external_index, font));
                        }
                    }
                }
            }
        }

        // Not a single match!
        None
    }

    /// Return the font previously loaded at this index.
    /// Panics if the index is not assigned.
    #[inline]
    pub fn get_with_index(&self, index: usize) -> Ref<Font> {
        let state = self.state.borrow();
        let internal = state.inner_index[index];
        Ref::map(state, |s| &s.fonts[internal].1)
    }
}

impl Debug for FontLoader<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("FontLoader")
            .field("providers", &self.providers.len())
            .field("fonts", &state.fonts)
            .field("query_cache", &state.query_cache)
            .field("info_cache", &state.info_cache)
            .field("inner_index", &state.inner_index)
            .finish()
    }
}

/// A query for a font with specific properties.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct FontQuery {
    /// Which character is needed.
    pub character: char,
    /// Which classes the font has to be part of.
    pub classes: Vec<FontClass>,
    /// The font matching the leftmost class in this sequence should be returned.
    pub fallback: Vec<FontClass>,
}
