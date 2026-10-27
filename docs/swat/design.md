# SWAT+ di RustGIS: dokumen desain

Status: draf untuk ditinjau · Fork: `andriyanto/rustgis` · Tanggal: 2026-10-22, diperbarui 2026-10-26

Dokumen ini mencatat keputusan untuk membawa seluruh alur kerja SWAT+ (GIS, editor, dan engine
simulasi) ke dalam RustGIS, di desktop dan di web. Fitur ini dikembangkan di fork dulu karena
ukurannya besar; belum ada rencana mengirimnya ke upstream `opengeos/RustGIS`.

## Ringkasan

- Engine SWAT+ (Fortran) diport ke Rust lebih dulu, divalidasi hari per hari terhadap build
  Fortran asli. GIS dan editor menyusul di atas fondasi itu.
- Engine adalah program terpisah berlisensi LGPL-2.1 (native dan modul WASM), karena merupakan
  terjemahan SWAT+. RustGIS (Apache-2.0) memanggilnya seperti alat `geolibre`, dan keduanya
  hanya bertukar format data.
- GIS dan editor di RustGIS berbagi satu **model proyek** dalam struct Rust. TxtInOut, PostGIS,
  dan Parquet hanyalah adapter penyimpanan untuk model itu.
- Delineasi DAS memakai alat Whitebox yang sudah ada di RustGIS (crate `geolibre`).
- Mode kolaborasi bersifat hybrid: antarmuka di browser, pengolahan DEM besar di desktop secara
  native, simulasi di worker server, dan hasil di server yang bisa dilihat siapa pun yang
  punya akses. Semua lewat server sebagai perantara.

## Keputusan

| # | Keputusan | Alasan |
| --- | --- | --- |
| K1 | Engine diport dan divalidasi lebih dulu, sebelum GIS dan editor | Risiko teknis terbesar ada di engine (~38.600 baris komputasi fisik). GIS sudah punya Whitebox. Engine bisa diuji tanpa GIS memakai data contoh SWAT+ |
| K2 | Model proyek dirancang sejak awal, bersama engine | Mencegah engine terikat ke TxtInOut, yang harus dibongkar lagi saat GIS dan database ditambahkan |
| K3 | TxtInOut tetap didukung sebagai adapter impor/ekspor | Wajib untuk validasi terhadap SWAT+ Fortran, dan agar proyek QSWAT+/SWAT+ Editor yang sudah ada bisa dibuka |
| K4 | Engine memakai `f32`, seperti `real` default SWAT+ | Build SWAT+ tidak memakai `-fdefault-real-8`; hanya 5 file memakai presisi ganda. Memakai `f64` akan membuat hasil tidak bisa dicocokkan |
| K5 | GIS memakai Whitebox lewat crate `geolibre` (geolibre-rust, dibangun di atas Whitebox Next Gen) | Sudah terintegrasi di RustGIS, native di desktop dan WASI di browser; lisensi MIT/Apache-2.0. Hanya alat open source yang dipakai, bukan paket Pro |
| K6 | Penyimpanan bersama di server: PostGIS untuk model dan layer spasial, Parquet/GeoParquet untuk output deret waktu | DuckDB adalah database tertanam dengan satu penulis, bukan server multi-pengguna. RustGIS sudah membaca PostGIS dan GeoParquet |
| K7 | DuckDB dipakai sebagai mesin analisis output: native di desktop/server, DuckDB-Wasm di browser, keduanya membaca Parquet | Kuat untuk query kolom besar; DuckDB-Wasm membaca Parquet lewat HTTP range, jadi browser hanya mengambil potongan yang perlu |
| K8 | Browser dan desktop tidak berkomunikasi langsung; keduanya lewat API dan antrean job di server | Browser membatasi halaman web yang memanggil `localhost`; job tetap aman saat desktop offline |
| K9 | Server bersifat opsional dan self-hosted | Tanpa server, RustGIS tetap bisa menjalankan seluruh alur secara lokal di desktop |
| K10 | Inti SWAT harus tetap bisa dibangun untuk `wasm32-unknown-unknown` | Aturan RustGIS "Never break wasm"; engine juga harus bisa jalan di browser untuk DAS kecil |
| K11 | Engine adalah program terpisah berlisensi LGPL-2.1 (native dan modul WASM), dipanggil RustGIS seperti `geolibre`; semua bagian RustGIS tetap Apache-2.0 | SWAT+ berlisensi LGPL-2.1 (tanpa klausul "or later"). Terjemahan engine adalah karya turunan. Batas proses/modul memisahkan kedua lisensi dengan bersih, dan pengguna bisa mengganti engine tanpa membangun ulang RustGIS (lihat [Lisensi dan batas engine](#lisensi-dan-batas-engine)) |
| K12 | Hasil Rust dianggap cocok bila selisih relatif setiap nilai harian ≤ 1e-4 (atau selisih absolut ≤ 1e-6) | Hasil identik sampai digit terakhir tidak mungkin antar-compiler. Ditinjau ulang setelah fase 2 |
| K13 | Perilaku SWAT+ yang janggal ditiru persis secara bawaan; perbaikan hanya lewat opsi yang dinyalakan sadar (`run --fix-swat`, daftarnya di `docs/swat-fixes.md`), dan semuanya dicatat di `docs/swat-quirks.md` repo engine | Tanpa itu hasil tidak bisa divalidasi terhadap Fortran |
| K14 | Acuan resmi adalah baseline build sendiri: SWAT+ commit `6f51536`, gfortran, Linux (workflow `swat-baseline.yml`). Commit dipatok selama port; pembaruan SWAT+ diterapkan terencana dengan baseline baru | Output acuan di repo SWAT+ berasal dari versi 2024 dan tidak lagi cocok dengan kodenya |

## Temuan dari kode SWAT+

Dasar: `swat-model/swatplus` commit `6f51536` (16 Okt 2026).

- 655 file sumber, 81.260 baris kode (tanpa komentar), 809 unit program: 628 subroutine,
  113 function, 67 module data global.
- Komposisi: 48% komputasi fisik, 23% pembacaan input dan inisialisasi, 23% penulisan output,
  6% definisi data.
- Loop harian: `main` → `time_control` (tahun × hari) → `command` (objek dalam urutan hulu ke
  hilir) → `hru_control`, `ru_control`, `aqu_1d_control`, `res_control`,
  `sd_channel_control3`, `gwflow_simulate`.
- Inti neraca air satu HRU sekitar 2.200 baris, iklim sekitar 630 baris.
- Jebakan porting:
    - 5.280 variabel lokal di 528 file diberi nilai awal saat deklarasi. Di Fortran ini membuat
      variabel bersifat `SAVE` (nilainya bertahan antar-pemanggilan), jadi tiap kasus harus
      diaudit.
    - State global di 67 module; variabel sementara seperti `sepday` dan `latlyr` dioper lewat
      module, bukan argumen.
    - 306 perintah `open` tersebar di 182 file.
    - Indeks array mulai dari 1; opsi build `-finit-local-zero` membuat lokal tanpa nilai awal
      bernilai nol.
- Sekitar 9 file tidak dipakai sama sekali (misalnya `ch_rtday`, `ch_rthr`,
  `channel_surf_link`); kanal klasik sudah digantikan `sd_channel` (SWAT-DEG).

## Arsitektur

### Alur hybrid

```text
  Web RustGIS (browser)                    Desktop RustGIS (native)
  antarmuka, kirim job, lihat hasil        ambil job, Whitebox native, DEM besar
            │                                         │
            └─────────────── HTTPS ───────────────────┘
                                │
  ┌─────────────────────────────┴─────────────────────────────────────┐
  │ Server (opsional, self-hosted)                                    │
  │   API + antrean job  ──  PostGIS (model, layer)  ──  Worker engine │
  │                          Parquet (output)                          │
  └────────────────────────────────────────────────────────────────────┘
```

1. Pengguna menyiapkan job delineasi di web (DEM, tutupan lahan, tanah, ambang sungai). Job
   masuk antrean di server.
2. Desktop RustGIS yang login ke server yang sama mengambil job, menjalankan Whitebox secara
   native, lalu mengunggah sungai, subDAS, dan HRU ke PostGIS.
3. Pengguna melengkapi model (iklim, parameter, jadwal manajemen) di editor, lalu mengirim job
   simulasi.
4. Worker engine di server menjalankan simulasi dan menulis output sebagai Parquet.
5. Siapa pun yang punya akses melihat hasilnya di web atau desktop.

Di mana setiap komponen bisa berjalan:

| Komponen | Desktop | Web (browser) | Server |
| --- | --- | --- | --- |
| Delineasi Whitebox | Ya, native, tanpa batas memori WASM | Ya, untuk DEM kecil (memori WASM ~4 GB) | Opsional nanti |
| Editor model | Ya | Ya | Tidak (lewat API) |
| Engine simulasi | Ya, native | Ya, untuk DAS kecil, di Web Worker | Ya, worker |
| Penyimpanan | TxtInOut, Parquet, DuckDB lokal | Memori/IndexedDB, Parquet lewat HTTP | PostGIS + Parquet |

### Lisensi dan batas engine

Engine SWAT versi Rust adalah terjemahan kode SWAT+ (LGPL-2.1), jadi ia dikembangkan sebagai
proyek tersendiri berlisensi LGPL-2.1, terpisah dari workspace RustGIS (Apache-2.0). Usulan nama
repo: `andriyanto/swatplus-rs`.

- **Batas yang memisahkan:** RustGIS tidak menautkan (link) kode engine. Natively RustGIS
  menjalankan program engine sebagai subprocess; di browser, web app memuat modul WASM engine
  saat dibutuhkan. Ini pola yang sama dengan `geolibre`.
- **Antarmuka berupa format data, bukan kode bersama.** RustGIS mengirim proyek dan menerima
  output dalam format yang terdokumentasi. Versi 1 memakai TxtInOut (folder, atau arsip tar di
  memori untuk browser; tar dipilih karena engine tidak boleh bergantung pada pustaka kompresi),
  karena kedua sisi memang harus bisa membacanya untuk validasi. Output dikirim balik sebagai
  Parquet, dan kemajuan simulasi sebagai baris JSON di stdout (seperti protokol process plugin
  RustGIS). Rinciannya di `docs/runner.md` repo engine.
- **Dependensi engine:** hanya crate berlisensi MIT atau dual MIT/Apache (memakai opsi MIT).
  Engine tidak bergantung pada crate `rustgis-*`, karena Apache-2.0 tidak kompatibel dengan
  LGPL-2.1-only menurut FSF.
- **Struktur internal engine** tetap mengikuti K2: model proyek dalam struct Rust, state
  simulasi terpisah, I/O hanya di adapter. Engine punya tipe modelnya sendiri; RustGIS punya
  `rustgis-swat-model` sendiri. Keduanya bertemu hanya lewat format data.
- **Tempat engine berjalan:** program native di desktop dan worker server, modul WASM di Web
  Worker browser. Satu basis kode, dua target build.

Catatan: ini kesimpulan teknis, bukan nasihat hukum. Sebelum rilis publik pertama, sebaiknya
ditinjau oleh orang yang paham lisensi open source.

### Crate baru di RustGIS

Semuanya Apache-2.0. Mengikuti lapisan RustGIS (`cargo xtask layers`): sebuah crate hanya boleh
bergantung pada lapisan di bawahnya, dan semua crate di bawah `apps/` harus bisa dibangun untuk
wasm32.

| Lapisan | Crate | Tugas |
| --- | --- | --- |
| L1 | `rustgis-swat-model` | Struct model proyek: objek, koneksi, tanah, lahan, manajemen, iklim, parameter, pengaturan output. Tanpa I/O |
| L2 | `rustgis-swat-txtinout` | Adapter TxtInOut: membaca dan menulis dari bytes/string, sehingga jalan di browser juga. Ditulis dari spesifikasi format file, bukan diterjemahkan dari kode baca SWAT+ |
| L3 | `rustgis-swat-runner` | Menjalankan engine: mencari atau mengunduh program engine natively (seperti `geolibre`), memuat modul WASM-nya di browser, meneruskan kemajuan dan pembatalan |
| L2 | `rustgis-swat-store` | Adapter PostGIS (tulis dan baca, desktop/server) dan Parquet (output) |
| L3 | `rustgis-swat-gis` | Delineasi dan pembentukan HRU, memanggil alat Whitebox lewat registri `rustgis-analysis` (berdasarkan id alat), bukan bergantung langsung pada `geolibre` |
| L4 | `rustgis-engine` (`cmd/swat.rs`) | Perintah `swat.*`: impor TxtInOut, delineasi, buat HRU, jalankan simulasi, ekspor |
| apps | `rustgis-swat-server` | API (misalnya `axum`), login, antrean job, worker engine |
| apps | `rustgis-cli swat agent` | Mode agen desktop: mengambil job dari server dan menjalankannya |

Karena semua aksi adalah perintah, SWAT otomatis bisa dipakai dari ribbon, CLI, ModelBuilder,
dan AI Assistant.

### Model proyek

Sketsa awal; bentuk akhirnya ditentukan saat porting iklim dan HRU (fase 1). Bentuk yang sama
dipakai `rustgis-swat-model` di RustGIS dan model internal engine, tetapi keduanya ditulis
terpisah karena berbeda lisensi.

```rust
pub struct Project {
    pub time: TimeSim,                 // time.sim
    pub print: PrintSettings,          // print.prt
    pub basin: BasinParams,            // codes.bsn, parameters.bsn
    pub objects: Vec<SpatialObject>,   // *.con: hru, ru, aqu, chandeg, res, recall...
    pub routing_order: Vec<ObjectId>,  // urutan hulu ke hilir, dihitung sekali
    pub hrus: Vec<HruData>,            // hru-data.hru dan tabel yang dirujuknya
    pub soils: Vec<Soil>,
    pub landuse: Vec<LandUse>,
    pub management: Vec<Schedule>,
    pub climate: Climate,              // stasiun, deret waktu, weather generator
    pub databases: Databases,          // plants.plt, fertilizer.frt, tillage.til...
}

pub struct SpatialObject {
    pub id: ObjectId,
    pub kind: ObjectKind,              // Hru, RoutingUnit, Aquifer, Channel, Reservoir...
    pub area_ha: f32,
    pub lat: f32,
    pub lon: f32,
    pub elev: f32,
    pub weather_station: StationId,
    pub outflows: Vec<Connection>,     // ke mana air mengalir
}

pub struct Connection {
    pub target: ObjectId,
    pub hyd: HydType,                  // tot, sur, lat, til, rhg, aqu...
    pub frac: f32,
}
```

State simulasi (`HruState`, `SoilState`, `ChannelState`, dan seterusnya) dipisahkan dari model,
sehingga satu model bisa dijalankan berkali-kali, misalnya untuk kalibrasi.

### Skema database (PostGIS)

Satu skema `swat`. Tabel model kurang lebih mengikuti file TxtInOut, sehingga impor dan ekspor
TxtInOut menjadi pemetaan langsung. Output deret waktu tidak disimpan di PostGIS, tetapi sebagai
file Parquet yang dicatat di `swat.output_file`.

```sql
CREATE SCHEMA swat;

CREATE TABLE swat.project (
  id          uuid PRIMARY KEY,
  name        text NOT NULL,
  crs         text NOT NULL,                -- misalnya 'EPSG:32748'
  created_by  text NOT NULL,
  created_at  timestamptz NOT NULL DEFAULT now(),
  time_sim    jsonb NOT NULL,               -- time.sim
  print_prt   jsonb NOT NULL                -- print.prt
);

CREATE TABLE swat.object (                  -- isi *.con
  project_id  uuid REFERENCES swat.project ON DELETE CASCADE,
  id          integer,
  kind        text NOT NULL,                -- hru | ru | aqu | chandeg | res | recall | ...
  name        text NOT NULL,
  area_ha     real NOT NULL,
  elev        real,
  station     text,
  geom        geometry(MultiPolygon),       -- batas HRU/subDAS; garis untuk kanal di tabel lain
  PRIMARY KEY (project_id, id)
);

CREATE TABLE swat.connection (              -- kolom out_tot, obj_typ, obj_id, hyd_typ, frac
  project_id  uuid,
  source_id   integer,
  seq         smallint,
  target_id   integer,
  hyd_type    text NOT NULL,
  frac        real NOT NULL,
  PRIMARY KEY (project_id, source_id, seq),
  FOREIGN KEY (project_id, source_id) REFERENCES swat.object
);

CREATE TABLE swat.channel_geom (
  project_id  uuid,
  object_id   integer,
  geom        geometry(MultiLineString),
  PRIMARY KEY (project_id, object_id)
);

CREATE TABLE swat.hru (                     -- hru-data.hru
  project_id  uuid,
  object_id   integer,
  topo        text, hydro text, soil text, lu_mgt text,
  soil_plant_init text, surf_stor text, snow text, field text,
  PRIMARY KEY (project_id, object_id)
);

-- Tabel parameter dan database bawaan (soils, landuse, management, plants, fertilizer,
-- tillage, ...) disimpan sebagai baris per entri dengan kolom jsonb, mengikuti versi file
-- SWAT+, agar skema tidak perlu berubah setiap SWAT+ menambah kolom.
CREATE TABLE swat.param_table (
  project_id  uuid REFERENCES swat.project ON DELETE CASCADE,
  file        text,                         -- misalnya 'soils.sol', 'plants.plt'
  name        text,
  value       jsonb NOT NULL,
  PRIMARY KEY (project_id, file, name)
);

CREATE TABLE swat.climate_station (
  project_id  uuid,
  name        text,
  geom        geometry(Point, 4326),
  wgn         jsonb,                        -- parameter weather generator
  PRIMARY KEY (project_id, name)
);

CREATE TABLE swat.climate_series (          -- pcp, tmp, slr, hmd, wnd
  project_id  uuid,
  station     text,
  variable    text,
  day         date,
  value       real,
  value2      real,                         -- tmax/tmin dan sejenisnya
  PRIMARY KEY (project_id, station, variable, day)
);

CREATE TABLE swat.job (
  id          uuid PRIMARY KEY,
  project_id  uuid REFERENCES swat.project ON DELETE CASCADE,
  kind        text NOT NULL,                -- delineate | build_hru | simulate | calibrate
  params      jsonb NOT NULL,
  status      text NOT NULL,                -- queued | running | done | failed | cancelled
  runner      text,                         -- desktop:<host> | server
  progress    real,
  message     text,
  created_by  text NOT NULL,
  created_at  timestamptz NOT NULL DEFAULT now(),
  started_at  timestamptz,
  finished_at timestamptz
);

CREATE TABLE swat.run (
  id          uuid PRIMARY KEY,
  project_id  uuid REFERENCES swat.project ON DELETE CASCADE,
  job_id      uuid REFERENCES swat.job,
  engine      text NOT NULL,                -- versi engine Rust
  model_hash  text NOT NULL,                -- hash model saat dijalankan
  created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE swat.output_file (             -- output deret waktu sebagai Parquet
  run_id      uuid REFERENCES swat.run ON DELETE CASCADE,
  table_name  text,                         -- hru_wb, channel_sd, aquifer, ...
  period      text,                         -- day | mon | yr | aa
  uri         text NOT NULL,                -- s3://... atau path lokal
  rows        bigint,
  PRIMARY KEY (run_id, table_name, period)
);
```

Hak akses (siapa boleh melihat atau mengubah proyek) memakai role PostgreSQL dan row-level
security per `project_id`; rinciannya diputuskan di fase 5.

## Validasi

- **Baseline Fortran:** workflow `.github/workflows/swat-baseline.yml` membangun SWAT+ pada
  commit yang dipatok, memeriksa build terhadap output acuan di repo SWAT+, lalu menjalankan
  `Osu_1hru` (1 HRU, 2010–2023) dan `Ames_sub1` (12 HRU) dengan output harian. Hasilnya
  disimpan sebagai artifact selama 90 hari.
- **Pembanding:** `tools/swat/compare_outputs.py` membandingkan tabel output dua run nilai per
  nilai dengan toleransi absolut dan relatif.
- **Toleransi awal:** relatif 1e-4 untuk neraca air harian. Karena keduanya memakai `f32`,
  perbedaan yang tersisa seharusnya hanya dari urutan operasi floating-point. Toleransi ini
  ditinjau ulang setelah fase 2.
- **Data uji tambahan:** kedua data contoh tidak memiliki jaringan routing yang besar (Ames tanpa
  kanal, Osu satu kanal). Untuk fase 3, Andriyanto menyiapkan proyek QSWAT+ dari wilayah
  Indonesia: folder TxtInOut hasil SWAT+ Editor beserta DEM, peta tutupan lahan, dan peta tanah
  yang dipakai. TxtInOut-nya untuk validasi engine; data spasialnya untuk membandingkan modul GIS
  (fase 7) dengan QSWAT+ pada masukan yang sama.

## Urutan kerja

| Fase | Isi | Selesai jika |
| --- | --- | --- |
| 0 | Baseline Fortran di CI, alat pembanding, dokumen ini | Artifact baseline tersedia untuk `Osu_1hru` dan `Ames_sub1` |
| 1 | Repo engine `swatplus-rs` (LGPL-2.1): model, pembaca TxtInOut, iklim dan weather generator | Model dari kedua data contoh terbaca lengkap; data iklim harian cocok |
| 2 | Neraca air HRU, bertahap di skenario turunan Ames (lihat `docs/phase2.md` di repo engine): 2a tanah bera tanpa tanaman dan manajemen ✅; 2b-1 padang rumput tanpa manajemen dengan model karbon asli (`cswat` 0): tanaman, hara N/P tanah, erosi ✅; 2b-2 padang rumput dengan model karbon Century (`cswat` 2) ✅ | 2a: `hru_wb` harian `Ames_bare` identik; 2b-1: `hru_wb`, `hru_pw`, `hru_nb`, `hru_ls` harian `Ames_past0` identik; 2b-2: tabel yang sama untuk `Ames_past` identik, karbon tanah per lapisan dalam presisi cetak |
| 3 | Objek spasial dan routing: loop `command`, routing unit, akuifer, kanal SWAT-DEG, bertahap pada varian proyek Batanghari dan Pessel (lihat `docs/phase3.md` di repo engine): 3a-0 lahan tropis Batanghari (`Bh_land`, 1.858 HRU, tabel keputusan tanam-panen generik, kalibrasi `perco`) ✅; 3a loop `command`, 284 routing unit, 44 akuifer ✅; 3b–3c 284 kanal SWAT-DEG seluruh jaringan dengan QUAL2E dan suhu air ✅; 3d proyek Batanghari utuh: PET Penman-Monteith, 167 HRU urban, 79 HRU lahan basah ✅; 3e Pessel: dataran rendah pesisir (`Ps_coast`, 20 DAS pesisir, 115 HRU di gambut, 74 kanal) ✅, Pessel utuh berikutnya | 3a-0: empat tabel HRU harian `Bh_land` identik; 3a: `ru` dan `aquifer` harian identik; 3b–3c: `channel_sd` harian identik; 3d: ketujuh tabel harian Batanghari identik; 3e: ketujuh tabel harian `Ps_coast` identik; Pessel utuh dalam toleransi |
| 4 | Manajemen, irigasi, nutrien, sedimen, karbon (`mgt_operatn`, tabel keputusan), lalu waduk dan lahan basah termasuk sawah (Osu). Sudah selesai lebih awal: M1 (operasi terjadwal pada jagung) ✅ dan M2 (komunitas tanaman, irigasi lewat tabel keputusan, lapisan tanah kustom; `Ames_sub1` utuh identik) ✅. Selesai berikutnya, masing-masing identik bit demi bit pada varian dataran rendah Pessel (lihat `docs/paddy.md`, `reservoirs.md`, `atmo.md`, `structures.md`, `nutrients.md` di repo engine): sawah dengan pematang, penggenangan, pelumpuran, pupuk kandang dan kalibrasi (`Osu_1hru` utuh) ✅; waduk dan sumber titik/recall (`Bh_res`) ✅; deposisi N atmosfer (`Ps_atmo`) dan routing Muskingum (`Ps_musk`) ✅; drainase bawah permukaan di gambut (`Ps_tile`, `Ames_tile`) ✅; filter strip, saluran berumput, BMP pengguna (`Ps_bmp`) ✅; tangki septik sebagaimana SWAT+ menjalankannya (`Ps_septic`) ✅; model P tanah dinamis (`Ps_solp`) ✅; beban urban regresi USGS (`Ps_urban`) ✅. langkah waktu sub-harian dengan infiltrasi Green-Ampt sampai debit kanal per jam (`Ps_hourly`, hujan harian dipecah per jam dengan distribusi Mononobe; lihat `docs/subdaily.md`) ✅ | `Ames_sub1` dan `Osu_1hru` utuh: `hru_wb`, `hru_nb`, `hru_ls`, `hru_pw`, hasil panen dalam toleransi; tiap varian fase 4 identik pada semua tabel |
| 4b | Engine 1.0 (di repo engine, lihat `docs/engine1.md`, `routing.md`, `subdaily.md`, `runner.md` di sana), masing-masing identik bit demi bit dengan SWAT+ pada varian ujinya: routing antarlanskap HRU ke HRU (`Ps_land`) ✅; HRU urban per jam (`Ps_urbhr`) ✅; PET "dibaca" (`Ps_pet`) ✅; kalibrasi bersyarat dan parameter kalibrasi yang lebih lengkap (`Ps_cal`) ✅; langkah per jam dengan curve number dan hidrograf satuan (`Ps_uh`) ✅; hujan per jam dari weather generator dan pengisian celah data per jam (`Ps_gap`) ✅; rilis waduk `natlake`, `dyrt1`, `inflo_targ`, `nonirr-h06`, `hydrop` (`Bh_rel`) ✅; semua jenis panen, operasi panen saja dan matikan (`Ps_harv`) ✅; hari kematangan tanaman tahunan dan populasi tanaman (`Ps_peren`) ✅; jadwal berdasarkan heat unit dan pengolahan tanah tanpa model karbon (`Ps_hu`) ✅; antarmuka runner: tabel Parquet tanpa dependensi, kemajuan sebagai baris JSON, `--strict` ✅; modul WASM (tar masuk, tar keluar, impor `swat_progress`) ✅; rilis CLI 1.0 (workflow rilis siap, dijalankan dengan tag `v1.0.0`). HRU paralel ditunda: beberapa variabel SWAT+ membawa nilai satu HRU ke HRU berikutnya, sehingga urutan HRU menentukan hasil; paralel hanya mungkin di mode koreksi (`--fix-swat`) setelah variabel itu dibuat per HRU. Engine satu thread sudah 2,6 kali lebih cepat dari SWAT+ pada Pessel utuh | Tiap fitur identik dengan SWAT+ pada varian ujinya; Pessel utuh berjalan; modul WASM menjalankan `Osu_1hru` (di CI lewat Node, mesin WebAssembly yang sama dengan browser) dan hasilnya sama dengan program native |
| 5 | `swat-store` (PostGIS tulis/baca, Parquet), server API, antrean job, worker | Simulasi berjalan dari job server, output terbaca di web |
| 6 | Editor di RustGIS (iklim, parameter, jadwal), perintah `swat.*`, panel SWAT di ribbon | Proyek bisa dibuat dan diedit tanpa SWAT+ Editor |
| 7 | `swat-gis`: delineasi dengan Whitebox, pembentukan HRU, mode agen desktop | Proyek baru dari DEM sampai simulasi tanpa QSWAT+ |
| 8 (opsional) | Pestisida, patogen, garam, konstituen, `gwflow`, alokasi air, kalibrasi | Sesuai kebutuhan |

Engine versi web (WASM di Web Worker) bisa mulai diuji sejak fase 2 selesai.

Catatan revisi (22 Okt 2026): kedua skenario acuan tidak bisa menguji neraca air secara terpisah. Osu adalah sawah dengan pematang dan tabel keputusan; Ames menanam jagung-kedelai dengan irigasi, pupuk, olah tanah, dan model karbon. Karena itu fase 2 divalidasi pada skenario turunan Ames yang dibuat oleh `scripts/make-variants.sh`, dan skenario acuan utuh dipindah ke fase 4.

## Risiko dan pertanyaan terbuka

- **Variabel implicit SAVE.** Bila ada yang terlewat saat audit, hasilnya menyimpang secara
  halus. Mitigasi: audit per routine sebelum diport, dan perbandingan harian.
- **SWAT+ terus berubah.** Port mengikuti satu commit yang dipatok; pembaruan dari upstream
  SWAT+ diterapkan secara sadar, dengan baseline baru.
- **Perbedaan numerik `f32`.** Fungsi matematika (`exp`, `log`) di Rust dan gfortran bisa
  berbeda di digit terakhir. Bila toleransi 1e-4 tidak tercapai, perlu dikaji per variabel.
- **Lisensi.** Sudah diputuskan di K11: engine terpisah berlisensi LGPL-2.1. Risiko yang
  tersisa adalah kebocoran batas: kode terjemahan SWAT+ tidak boleh masuk ke crate RustGIS
  (misalnya menyalin rumus dari engine ke modul editor). Rumus yang dibutuhkan di sisi RustGIS
  ditulis ulang dari dokumentasi teori SWAT, bukan dari kode engine.
- **Format QSWAT+ dan SWAT+ Editor.** Kode keduanya (Python) belum dipetakan. Ini diperlukan
  sebelum fase 6 dan 7 agar logika pembentukan HRU dan nilai bawaan editor sama.
- **Data uji besar.** Terpenuhi (26 Okt 2026): dua proyek nyata dari Sumatera Barat ada di
  `testdata/` repo engine, masing-masing dengan acuan Fortran yang identik dengan run SWAT+ rev.
  62.0.1 pemiliknya. `Batanghari` (DAS hulu bergunung, 42 subbasin, 284 kanal, 1.858 HRU) dan
  `Pessel` (DAS pesisir, 230 subbasin, 1.454 kanal, 3.966 HRU, gambut, sawah, lahan basah).
  Perilaku SWAT+ yang ditemukan saat menyusunnya (tanaman tahunan yang tidak pernah dorman di
  bawah lintang 20°, `perco`, nilai bawaan SWAT+ Editor) dicatat di `docs/swat-quirks.md`.
