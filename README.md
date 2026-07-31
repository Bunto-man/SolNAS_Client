# SolNAS Client

#### The sister application to the standalone program SolNAS.

**(Get ready to read)**
# Features

- Remotely change the config of SolNAS
- Customizable UI
- Easy Dashboard Management
- Multi-upload / Download
- Improved User Control
- Come on, it's better in every way.

--- 

### How to Obtain SolNAS Client:

**You have two options:**

1. (RECOMMENDED) If You are on windows, Go to the releases page, **download the RAR and extract**. If you are on Linux or a different OS, You might be able to utilize the Linux Mint release zip file. If not, fall back to option two.
2. Option two is to pull the git repository. Follow these instructions from your terminal:

```
# Ensure you have rust downloaded
rustup
```

> If you see an error or nothing, get rust. 
> https://rust-lang.org/tools/install/

```
# Go make a directory for the github.
mkdir SolNAS_Client
cd SolNAS_Client
git clone https://github.com/Bunto-man/SolNAS_Client.git
```

> Next, you will need to use the cargo commands to build your executible.

```
# Assuming you are in the github directory:
cargo build --release
# Your exe file or your executible will be in /target/release
```
- I will leave making a custom launcher up to you. It's good practice on linux.
- EXE files work out of the box on windows.

### Using the Client Program

**I tried to make everything easy.** Simply run the EXE file or the launcher, and you will be greeted by the dashboard password screen. The password is the password chosen for the SolNAS program. *The IP address is the address that SolNAS gives you ( 192.xxx.x.xxx) or whatever you have. *

--- 
### Use Cases

> Using SolNAS Client gives you better control over the SolNAS program.

- Use the Server config button to change upload speed and max upload file size.
- Use the move buttons to shuffle folders and files to different directories.
- Use the add folder button to add a folder of your choice.
- Use the delete button to delete files and entire folders.
- Multiple uploads and downloads are supported by the client program, just like the website version.
- Colors of the dashboard and buttons can be controlled.
### Customization

**Everyone loves personalization**

- A file called style.ini is generated when first running the client. 
- The colors of the program are controlled in this file
- Each of the colors are stored in hexadecimal, replace the colors and re-run the program.

`Google 'hex color' , choose your colors, copy and paste. Voila.`

### The Ram Update Fix

The RAM update brought an issue with dependencies, so those who are on linux may need to install new dependencies. 

These are the dependencies that need to be installed:

```
sudo apt update && sudo apt install libx11-dev libxcursor-dev libxrandr-dev libxi-dev libvulkan-dev libwayland-dev libxkbcommon-dev libdbus-1-dev libxdo-dev
```

---

**These are only needed because of a change I made to the way the app works**

#### Read this!:

**The app will *NOT* close upon pressing the close button. You must either kill the process or use the system tray icon to close it.**

This change prevents the app from closing, obviously, since I personally do it out of habit.
The app should stay open. It uses only about 80MB of RAM (on windows) without any images loaded.
