from PyInstaller.utils.hooks import collect_data_files, collect_submodules


datas = collect_data_files("librosa", excludes=["**/__pycache__"])
hiddenimports = collect_submodules("librosa")
module_collection_mode = "pyz+py"
