use std::fmt::{Display, Formatter};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::{tempdir, TempDir};

pub use canonical_url::{CanonicalUrl, RepositoryUrl};
#[cfg(feature = "clap")]
pub use cli::CacheArgs;
pub use digest::digest;
pub use metadata::WheelMetadataCache;
pub use stable_hash::{StableHash, StableHasher};

mod cache_key;
mod canonical_url;
mod cli;
mod digest;
mod metadata;
mod stable_hash;

/// A cache entry which may or may not exist yet.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub dir: PathBuf,
    pub file: String,
}

impl CacheEntry {
    pub fn path(&self) -> PathBuf {
        // TODO(konstin): Cache this to avoid allocations?
        self.dir.join(&self.file)
    }
}

/// The main cache abstraction.
#[derive(Debug, Clone)]
pub struct Cache {
    /// The cache directory.
    root: PathBuf,
    /// A temporary cache directory, if the user requested `--no-cache`.
    ///
    /// Included to ensure that the temporary directory exists for the length of the operation, but
    /// is dropped at the end as appropriate.
    _temp_dir_drop: Option<Arc<TempDir>>,
}

impl Cache {
    /// A persistent cache directory at `root`.
    pub fn from_path(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            _temp_dir_drop: None,
        }
    }

    /// Create a temporary cache directory.
    pub fn temp() -> Result<Self, io::Error> {
        let temp_dir = tempdir()?;
        Ok(Self {
            root: temp_dir.path().to_path_buf(),
            _temp_dir_drop: Some(Arc::new(temp_dir)),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder for a specific cache bucket
    pub fn bucket(&self, cache_bucket: CacheBucket) -> PathBuf {
        self.root.join(cache_bucket.to_str())
    }

    pub fn entry(
        &self,
        cache_bucket: CacheBucket,
        dir: impl AsRef<Path>,
        file: String,
    ) -> CacheEntry {
        CacheEntry {
            dir: self.bucket(cache_bucket).join(dir.as_ref()),
            file,
        }
    }
}

/// The different kinds of data in the cache are stored in different bucket, which in our case
/// are subfolders.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum CacheBucket {
    /// Downloaded remote wheel archives.
    Archives,
    /// Metadata of a built source distribution.
    ///
    /// Cache structure:
    ///  * `<build wheel metadata cache>/pypi/foo-1.0.0.zip/metadata.json`
    ///  * `<build wheel metadata cache>/<sha256(index-url)>/foo-1.0.0.zip/metadata.json`
    ///  * `<build wheel metadata cache>/url/<sha256(url)>/foo-1.0.0.zip/metadata.json`
    ///
    /// But the url filename does not need to be a valid source dist filename
    /// (<https://github.com/search?q=path%3A**%2Frequirements.txt+master.zip&type=code>),
    /// so it could also be the following and we have to take any string as filename:
    ///  * `<build wheel metadata cache>/url/<sha256(url)>/master.zip/metadata.json`
    ///
    /// # Example
    ///
    /// The following requirements:
    /// ```text
    /// # git source dist
    /// pydantic-extra-types @ git+https://github.com/pydantic/pydantic-extra-types.git    
    /// # pypi source dist
    /// django_allauth==0.51.0
    /// # url source dist
    /// werkzeug @ https://files.pythonhosted.org/packages/0d/cc/ff1904eb5eb4b455e442834dabf9427331ac0fa02853bf83db817a7dd53d/werkzeug-3.0.1.tar.gz
    /// ```
    ///
    /// ...may be cached as:
    /// ```text
    /// built-wheel-metadata-v0
    /// ├── git
    /// │   └── 5c56bc1c58c34c11
    /// │       └── 843b753e9e8cb74e83cac55598719b39a4d5ef1f
    /// │           └── metadata.json
    /// ├── pypi
    /// │   └── django-allauth-0.51.0.tar.gz
    /// │       └── metadata.json
    /// └── url
    ///     └── 6781bd6440ae72c2
    ///         └── werkzeug-3.0.1.tar.gz
    ///             └── metadata.json
    /// ```
    ///
    /// The inside of a `metadata.json`:
    /// ```json
    /// {
    ///   "data": {
    ///     "django_allauth-0.51.0-py3-none-any.whl": {
    ///       "metadata-version": "2.1",
    ///       "name": "django-allauth",
    ///       "version": "0.51.0",
    ///       ...
    ///     }
    ///   }
    /// }
    /// ```
    BuiltWheelMetadata,
    /// Wheel archives built from source distributions.
    BuiltWheels,
    /// Git repositories.
    Git,
    /// Information about an interpreter at a path.
    Interpreter,
    /// Index responses through the simple metadata API.
    Simple,
    /// Metadata of a remote wheel.
    ///
    /// Cache structure:
    ///  * `wheel-metadata-v0/pypi/foo-1.0.0-py3-none-any.json`
    ///  * `wheel-metadata-v0/<digest(index-url)>/foo-1.0.0-py3-none-any.json`
    ///  * `wheel-metadata-v0/url/<digest(url)>/foo-1.0.0-py3-none-any.json`
    ///
    /// See `puffin_client::RegistryClient::wheel_metadata` for information on how wheel metadata
    /// is fetched.
    ///
    /// # Example
    ///
    /// The following requirements:
    /// ```text
    /// # pypi wheel
    /// pandas
    /// # url wheel
    /// flask @ https://files.pythonhosted.org/packages/36/42/015c23096649b908c809c69388a805a571a3bea44362fe87e33fc3afa01f/flask-3.0.0-py3-none-any.whl
    /// ```
    ///
    /// ...may be cached as:
    /// ```text
    /// wheel-metadata-v0
    /// ├── pypi
    /// │   ...
    /// │   ├── pandas-2.1.3-cp310-cp310-manylinux_2_17_x86_64.manylinux2014_x86_64.json
    /// │   ...
    /// └── url
    ///     └── 4b8be67c801a7ecb
    ///         └── flask-3.0.0-py3-none-any.json
    /// ```
    WheelMetadata,
    /// Unzipped wheels, ready for installation via reflinking, symlinking, or copying.
    Wheels,
}

impl CacheBucket {
    fn to_str(self) -> &'static str {
        match self {
            CacheBucket::Archives => "archives-v0",
            CacheBucket::BuiltWheelMetadata => "built-wheel-metadata-v0",
            CacheBucket::BuiltWheels => "built-wheels-v0",
            CacheBucket::Git => "git-v0",
            CacheBucket::Interpreter => "interpreter-v0",
            CacheBucket::Simple => "simple-v0",
            CacheBucket::WheelMetadata => "wheel-metadata-v0",
            CacheBucket::Wheels => "wheels-v0",
        }
    }
}

impl Display for CacheBucket {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_str())
    }
}
