#[derive(Debug, Clone)]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub last_polled_at: Option<i64>,
    pub last_status: Option<String>,
    pub added_at: i64,
    pub unread_count: u32,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: i64,
    pub feed_id: i64,
    pub guid: String,
    pub title: String,
    pub link: Option<String>,
    pub author: Option<String>,
    pub published_at: Option<i64>,
    pub fetched_at: i64,
    pub content_html: Option<String>,
    pub is_read: bool,
}

/// Item without `content_html`, for cheap list views.
#[derive(Debug, Clone)]
pub struct ItemSummary {
    pub id: i64,
    pub feed_id: i64,
    pub title: String,
    pub link: Option<String>,
    pub author: Option<String>,
    pub published_at: Option<i64>,
    pub fetched_at: i64,
    pub is_read: bool,
}

/// A parsed feed item that has not been stored yet.
#[derive(Debug, Clone)]
pub struct NewItem {
    pub guid: String,
    pub title: String,
    pub link: Option<String>,
    pub author: Option<String>,
    pub published_at: Option<i64>,
    pub content_html: Option<String>,
}
