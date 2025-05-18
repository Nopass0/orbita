# План разработки файловой системы (VFS + FAT32)

## Общая информация

Файловая система необходима для:
- Хранения и организации данных
- Загрузки программ и библиотек
- Сохранения настроек и данных пользователя

Реализуем:
1. VFS (Virtual File System) - абстракцию для работы с файлами
2. FAT32 - простую и популярную файловую систему

## Этапы разработки

### Этап 1: VFS интерфейс

1. **Создать файл**: `fs/vfs/mod.rs`

2. **Основные трейты**:
   ```rust
   pub trait FileSystem {
       fn mount(&mut self, device: &dyn BlockDevice) -> Result<(), FsError>;
       fn unmount(&mut self) -> Result<(), FsError>;
       fn root(&self) -> Arc<dyn Inode>;
   }
   
   pub trait Inode: Send + Sync {
       fn metadata(&self) -> Result<Metadata, FsError>;
       fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError>;
       fn write(&self, offset: usize, buffer: &[u8]) -> Result<usize, FsError>;
       fn lookup(&self, name: &str) -> Result<Arc<dyn Inode>, FsError>;
       fn create(&self, name: &str, mode: FileMode) -> Result<Arc<dyn Inode>, FsError>;
       fn unlink(&self, name: &str) -> Result<(), FsError>;
   }
   
   pub trait BlockDevice: Send + Sync {
       fn read_block(&self, block: u64, buffer: &mut [u8]) -> Result<(), IoError>;
       fn write_block(&self, block: u64, buffer: &[u8]) -> Result<(), IoError>;
       fn block_size(&self) -> usize;
       fn total_blocks(&self) -> u64;
   }
   ```

3. **Структуры метаданных**:
   ```rust
   #[derive(Debug, Clone)]
   pub struct Metadata {
       pub file_type: FileType,
       pub size: u64,
       pub permissions: u16,
       pub created: DateTime,
       pub modified: DateTime,
       pub accessed: DateTime,
   }
   
   #[derive(Debug, Clone, Copy, PartialEq)]
   pub enum FileType {
       File,
       Directory,
       Symlink,
       Device,
   }
   
   #[derive(Debug, Clone, Copy)]
   pub struct FileMode {
       pub read: bool,
       pub write: bool,
       pub execute: bool,
   }
   ```

### Этап 2: FAT32 структуры

1. **Создать файл**: `fs/fat32/mod.rs`

2. **Структуры загрузочного сектора**:
   ```rust
   #[repr(C, packed)]
   pub struct Fat32BootSector {
       jump_boot: [u8; 3],
       oem_name: [u8; 8],
       bytes_per_sector: u16,
       sectors_per_cluster: u8,
       reserved_sectors: u16,
       num_fats: u8,
       root_entries: u16,
       total_sectors_16: u16,
       media_type: u8,
       fat_size_16: u16,
       sectors_per_track: u16,
       num_heads: u16,
       hidden_sectors: u32,
       total_sectors_32: u32,
       // FAT32 специфичные поля
       fat_size_32: u32,
       ext_flags: u16,
       fs_version: u16,
       root_cluster: u32,
       fs_info: u16,
       backup_boot_sector: u16,
       reserved: [u8; 12],
       drive_number: u8,
       reserved1: u8,
       boot_signature: u8,
       volume_id: u32,
       volume_label: [u8; 11],
       fs_type: [u8; 8],
   }
   ```

3. **Структура директорий**:
   ```rust
   #[repr(C, packed)]
   pub struct Fat32DirEntry {
       name: [u8; 11],
       attr: u8,
       nt_res: u8,
       crt_time_tenth: u8,
       crt_time: u16,
       crt_date: u16,
       lst_acc_date: u16,
       fst_clus_hi: u16,
       wrt_time: u16,
       wrt_date: u16,
       fst_clus_lo: u16,
       file_size: u32,
   }
   
   const ATTR_READ_ONLY: u8 = 0x01;
   const ATTR_HIDDEN: u8 = 0x02;
   const ATTR_SYSTEM: u8 = 0x04;
   const ATTR_VOLUME_ID: u8 = 0x08;
   const ATTR_DIRECTORY: u8 = 0x10;
   const ATTR_ARCHIVE: u8 = 0x20;
   const ATTR_LONG_NAME: u8 = 0x0F;
   ```

### Этап 3: Чтение FAT таблицы

1. **Управление кластерами**:
   ```rust
   pub struct Fat32 {
       boot_sector: Fat32BootSector,
       fat_start_sector: u32,
       data_start_sector: u32,
       device: Arc<dyn BlockDevice>,
       fat_cache: HashMap<u32, Vec<u32>>,
   }
   
   impl Fat32 {
       fn cluster_to_sector(&self, cluster: u32) -> u32 {
           ((cluster - 2) * self.boot_sector.sectors_per_cluster as u32) + self.data_start_sector
       }
       
       fn get_next_cluster(&self, cluster: u32) -> Result<Option<u32>, FsError> {
           let fat_offset = cluster * 4;
           let fat_sector = self.fat_start_sector + (fat_offset / self.boot_sector.bytes_per_sector as u32);
           let fat_offset_in_sector = (fat_offset % self.boot_sector.bytes_per_sector as u32) as usize;
           
           let mut buffer = vec![0u8; self.boot_sector.bytes_per_sector as usize];
           self.device.read_block(fat_sector as u64, &mut buffer)?;
           
           let next_cluster = u32::from_le_bytes([
               buffer[fat_offset_in_sector],
               buffer[fat_offset_in_sector + 1],
               buffer[fat_offset_in_sector + 2],
               buffer[fat_offset_in_sector + 3],
           ]) & 0x0FFFFFFF;
           
           match next_cluster {
               0x0000000..=0x0000001 => Ok(None), // Свободный кластер
               0x0FFFFFF8..=0x0FFFFFFF => Ok(None), // Конец цепочки
               _ => Ok(Some(next_cluster)),
           }
       }
   }
   ```

### Этап 4: Чтение директорий

1. **Парсинг записей директории**:
   ```rust
   fn read_directory(&self, cluster: u32) -> Result<Vec<Fat32DirEntry>, FsError> {
       let mut entries = Vec::new();
       let mut current_cluster = Some(cluster);
       
       while let Some(cluster) = current_cluster {
           let sector = self.cluster_to_sector(cluster);
           let sectors_per_cluster = self.boot_sector.sectors_per_cluster;
           
           for i in 0..sectors_per_cluster {
               let mut buffer = vec![0u8; self.boot_sector.bytes_per_sector as usize];
               self.device.read_block((sector + i as u32) as u64, &mut buffer)?;
               
               for chunk in buffer.chunks_exact(32) {
                   let entry = unsafe { 
                       ptr::read(chunk.as_ptr() as *const Fat32DirEntry) 
                   };
                   
                   if entry.name[0] == 0x00 {
                       return Ok(entries); // Конец директории
                   }
                   
                   if entry.name[0] != 0xE5 { // Не удаленная запись
                       entries.push(entry);
                   }
               }
           }
           
           current_cluster = self.get_next_cluster(cluster)?;
       }
       
       Ok(entries)
   }
   ```

2. **Обработка длинных имен**:
   ```rust
   #[repr(C, packed)]
   struct LongNameEntry {
       order: u8,
       name1: [u16; 5],
       attr: u8,
       type_: u8,
       checksum: u8,
       name2: [u16; 6],
       cluster: u16,
       name3: [u16; 2],
   }
   
   fn parse_long_name(entries: &[Fat32DirEntry]) -> (String, usize) {
       let mut name = String::new();
       let mut i = 0;
       
       while i < entries.len() && entries[i].attr == ATTR_LONG_NAME {
           let long_entry = unsafe {
               ptr::read(&entries[i] as *const _ as *const LongNameEntry)
           };
           
           // Собираем имя из фрагментов
           let mut chars = Vec::new();
           chars.extend_from_slice(&long_entry.name1);
           chars.extend_from_slice(&long_entry.name2);
           chars.extend_from_slice(&long_entry.name3);
           
           for ch in chars {
               if ch == 0 || ch == 0xFFFF { break; }
               name.push(char::from_u32(ch as u32).unwrap_or('?'));
           }
           
           i += 1;
           
           if long_entry.order & 0x40 != 0 { break; } // Последняя часть
       }
       
       (name, i)
   }
   ```

### Этап 5: Чтение и запись файлов

1. **Чтение файла**:
   ```rust
   fn read_file(&self, dir_entry: &Fat32DirEntry, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
       let first_cluster = ((dir_entry.fst_clus_hi as u32) << 16) | (dir_entry.fst_clus_lo as u32);
       let file_size = dir_entry.file_size as usize;
       
       if offset >= file_size {
           return Ok(0);
       }
       
       let bytes_to_read = buffer.len().min(file_size - offset);
       let bytes_per_cluster = self.boot_sector.bytes_per_sector as usize * 
                               self.boot_sector.sectors_per_cluster as usize;
       
       let start_cluster_offset = offset / bytes_per_cluster;
       let offset_in_cluster = offset % bytes_per_cluster;
       
       let mut current_cluster = Some(first_cluster);
       let mut cluster_index = 0;
       
       // Пропускаем кластеры до нужного
       while cluster_index < start_cluster_offset {
           current_cluster = self.get_next_cluster(current_cluster.unwrap())?;
           cluster_index += 1;
       }
       
       let mut bytes_read = 0;
       
       while bytes_read < bytes_to_read && current_cluster.is_some() {
           let cluster = current_cluster.unwrap();
           let sector = self.cluster_to_sector(cluster);
           
           // Читаем нужную часть кластера
           let offset_in_current = if cluster_index == start_cluster_offset {
               offset_in_cluster
           } else {
               0
           };
           
           let bytes_in_cluster = bytes_per_cluster - offset_in_current;
           let bytes_to_read_now = (bytes_to_read - bytes_read).min(bytes_in_cluster);
           
           // Читаем секторы кластера
           self.read_cluster_data(sector, offset_in_current, 
                                &mut buffer[bytes_read..bytes_read + bytes_to_read_now])?;
           
           bytes_read += bytes_to_read_now;
           current_cluster = self.get_next_cluster(cluster)?;
           cluster_index += 1;
       }
       
       Ok(bytes_read)
   }
   ```

2. **Запись файла**:
   ```rust
   fn write_file(&mut self, dir_entry: &mut Fat32DirEntry, offset: usize, buffer: &[u8]) -> Result<usize, FsError> {
       // Аналогично чтению, но с записью
       // Нужно обновить размер файла и выделить новые кластеры при необходимости
       todo!()
   }
   ```

### Этап 6: Создание и удаление файлов

1. **Создание файла**:
   ```rust
   fn create_file(&mut self, parent_cluster: u32, name: &str) -> Result<Fat32DirEntry, FsError> {
       // 1. Проверить, не существует ли файл
       // 2. Найти свободную запись в директории
       // 3. Выделить первый кластер для файла
       // 4. Создать запись директории
       // 5. Записать изменения
       todo!()
   }
   ```

### Этап 7: Реализация VFS для FAT32

1. **Структура FAT32 Inode**:
   ```rust
   struct Fat32Inode {
       fs: Arc<Mutex<Fat32>>,
       entry: Fat32DirEntry,
       path: PathBuf,
   }
   
   impl Inode for Fat32Inode {
       fn metadata(&self) -> Result<Metadata, FsError> {
           Ok(Metadata {
               file_type: if self.entry.attr & ATTR_DIRECTORY != 0 {
                   FileType::Directory
               } else {
                   FileType::File
               },
               size: self.entry.file_size as u64,
               permissions: 0o755, // FAT32 не поддерживает права доступа
               created: fat_datetime_to_unix(self.entry.crt_date, self.entry.crt_time),
               modified: fat_datetime_to_unix(self.entry.wrt_date, self.entry.wrt_time),
               accessed: fat_datetime_to_unix(self.entry.lst_acc_date, 0),
           })
       }
       
       fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
           self.fs.lock().read_file(&self.entry, offset, buffer)
       }
       
       // ... остальные методы
   }
   ```

### Этап 8: Интеграция с системой

1. **Менеджер файловых систем**:
   ```rust
   pub struct VfsManager {
       filesystems: HashMap<String, Arc<dyn FileSystem>>,
       mounts: HashMap<PathBuf, Arc<dyn Inode>>,
   }
   
   impl VfsManager {
       pub fn mount(&mut self, fs_type: &str, device: Arc<dyn BlockDevice>, mount_point: &Path) -> Result<(), FsError> {
           let fs = match fs_type {
               "fat32" => {
                   let mut fat32 = Fat32::new();
                   fat32.mount(device.as_ref())?;
                   Arc::new(fat32) as Arc<dyn FileSystem>
               }
               _ => return Err(FsError::UnsupportedFilesystem),
           };
           
           let root = fs.root();
           self.mounts.insert(mount_point.to_path_buf(), root);
           self.filesystems.insert(mount_point.to_string_lossy().to_string(), fs);
           
           Ok(())
       }
   }
   ```

## Проверочный список для AI-агентов

Перед завершением проверьте:

1. ✓ Правильное выравнивание структур (`#[repr(C, packed)]`)
2. ✓ Обработка big/little endian
3. ✓ Проверка границ при чтении/записи
4. ✓ Корректная работа с кэшем
5. ✓ Обработка фрагментированных файлов
6. ✓ Поддержка длинных имен файлов
7. ✓ Атомарность операций записи

## Тестирование

1. Чтение существующей FAT32 файловой системы
2. Создание и удаление файлов
3. Запись больших файлов
4. Работа с глубокой вложенностью директорий
5. Стресс-тест с множеством операций

## Зависимости

- Требует драйвер диска (ATA/AHCI)
- Использует блочное устройство
- Необходим для загрузки программ