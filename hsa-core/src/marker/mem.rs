
#[hsair_lang_item = "global_mem"]
pub struct Global<T>(T);
#[hsair_lang_item = "shared_mem"]
pub struct Shared<T>(T);

#[hsair_lang_item = "read_mem"]
pub struct Read<T>(T);
#[hsair_lang_item = "readwrite_mem"]
pub struct ReadWrite<T>(T);
#[hsair_lang_item = "write_mem"]
pub struct Write<T>(T);

