//go:build windows

package main

import (
	_ "embed"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
	"unsafe"
)

//go:embed assets/imdb.ico
var trayIconBytes []byte

const (
	wmDestroy      = 0x0002
	wmClose        = 0x0010
	wmNull         = 0x0000
	wmLButtonUp    = 0x0202
	wmLButtonDblCl = 0x0203
	wmRButtonUp    = 0x0205
	wmUser         = 0x0400
	trayCallback   = wmUser + 77
	trayRestore    = wmUser + 78
	ninSelect      = wmUser
	ninKeySelect   = wmUser + 1

	nimAdd                             = 0x00000000
	nimDelete                          = 0x00000002
	nimSetVersion                      = 0x00000004
	nifMessage                         = 0x00000001
	nifIcon                            = 0x00000002
	nifTip                             = 0x00000004
	notifyVersion4                     = 4
	ERROR_ALREADY_EXISTS syscall.Errno = 183

	imageIcon      = 1
	lrLoadFromFile = 0x00000010
	lrDefaultSize  = 0x00000040

	swHide    = 0
	swShow    = 5
	swRestore = 9

	mfString       = 0x00000000
	tpmRightButton = 0x0002
	tpmReturnCmd   = 0x0100

	menuRestore = 1001
	menuExit    = 1002
)

type winPoint struct {
	X int32
	Y int32
}

type winMsg struct {
	HWnd     uintptr
	Message  uint32
	WParam   uintptr
	LParam   uintptr
	Time     uint32
	Pt       winPoint
	LPrivate uint32
}

type wndClassEx struct {
	CbSize        uint32
	Style         uint32
	LpfnWndProc   uintptr
	CbClsExtra    int32
	CbWndExtra    int32
	HInstance     uintptr
	HIcon         uintptr
	HCursor       uintptr
	HbrBackground uintptr
	LpszMenuName  *uint16
	LpszClassName *uint16
	HIconSm       uintptr
}

type notifyIconData struct {
	CbSize           uint32
	HWnd             uintptr
	UID              uint32
	UFlags           uint32
	UCallbackMessage uint32
	HIcon            uintptr
	SzTip            [128]uint16
	DwState          uint32
	DwStateMask      uint32
	SzInfo           [256]uint16
	UVersion         uint32
	SzInfoTitle      [64]uint16
	DwInfoFlags      uint32
	GuidItem         [16]byte
	HBalloonIcon     uintptr
}

var (
	user32             = syscall.NewLazyDLL("user32.dll")
	kernel32           = syscall.NewLazyDLL("kernel32.dll")
	shell32            = syscall.NewLazyDLL("shell32.dll")
	procRegisterClass  = user32.NewProc("RegisterClassExW")
	procCreateWindow   = user32.NewProc("CreateWindowExW")
	procDefWindowProc  = user32.NewProc("DefWindowProcW")
	procGetMessage     = user32.NewProc("GetMessageW")
	procTranslateMsg   = user32.NewProc("TranslateMessage")
	procDispatchMsg    = user32.NewProc("DispatchMessageW")
	procPostQuitMsg    = user32.NewProc("PostQuitMessage")
	procEnumWindows    = user32.NewProc("EnumWindows")
	procGetWindowTextL = user32.NewProc("GetWindowTextLengthW")
	procGetWindowText  = user32.NewProc("GetWindowTextW")
	procGetWindowPID   = user32.NewProc("GetWindowThreadProcessId")
	procFindWindow     = user32.NewProc("FindWindowW")
	procIsWindow       = user32.NewProc("IsWindow")
	procIsIconic       = user32.NewProc("IsIconic")
	procShowWindow     = user32.NewProc("ShowWindow")
	procSetForeground  = user32.NewProc("SetForegroundWindow")
	procPostMessage    = user32.NewProc("PostMessageW")
	procGetCursorPos   = user32.NewProc("GetCursorPos")
	procCreateMenu     = user32.NewProc("CreatePopupMenu")
	procAppendMenu     = user32.NewProc("AppendMenuW")
	procTrackPopupMenu = user32.NewProc("TrackPopupMenu")
	procDestroyMenu    = user32.NewProc("DestroyMenu")
	procLoadImage      = user32.NewProc("LoadImageW")
	procGetModule      = kernel32.NewProc("GetModuleHandleW")
	procCreateMutexW   = kernel32.NewProc("CreateMutexW")
	procReleaseMutex   = kernel32.NewProc("ReleaseMutex")
	procCloseHandle    = kernel32.NewProc("CloseHandle")
	procShellNotify    = shell32.NewProc("Shell_NotifyIconW")
)

func platformAcquireManagerInstance() (release func(), primaryInstance bool, err error) {
	name, _ := syscall.UTF16PtrFromString("Local\\TechCardManagerPortableGUI400")
	handle, _, callErr := procCreateMutexW.Call(0, 1, uintptr(unsafe.Pointer(name)))
	if handle == 0 {
		return func() {}, false, fmt.Errorf("CreateMutexW: %v", callErr)
	}
	if callErr == ERROR_ALREADY_EXISTS {
		for attempt := 0; attempt < 40; attempt++ {
			if requestExistingManagerRestore() {
				break
			}
			time.Sleep(50 * time.Millisecond)
		}
		procCloseHandle.Call(handle)
		return func() {}, false, nil
	}
	return func() {
		procReleaseMutex.Call(handle)
		procCloseHandle.Call(handle)
	}, true, nil
}

var trayState struct {
	sync.Mutex
	started        bool
	hostHwnd       uintptr
	appHwnd        uintptr
	icon           uintptr
	iconPath       string
	url            string
	quit           chan struct{}
	windowClosed   chan struct{}
	exitRequested  chan struct{}
	stopWatch      chan struct{}
	exitInProgress bool
	trayIconReady  bool
	quitOnce       sync.Once
}

type traySignals struct {
	Quit          <-chan struct{}
	WindowClosed  <-chan struct{}
	ExitRequested <-chan struct{}
}

var enumeratedManagerWindow atomic.Uintptr

var enumManagerWndProc = syscall.NewCallback(func(hwnd, _ uintptr) uintptr {
	var windowPID uint32
	procGetWindowPID.Call(hwnd, uintptr(unsafe.Pointer(&windowPID)))
	if !platformOwnsAppWindowPID(int(windowPID)) {
		return 1
	}
	length, _, _ := procGetWindowTextL.Call(hwnd)
	if length == 0 {
		return 1
	}
	buf := make([]uint16, length+1)
	procGetWindowText.Call(hwnd, uintptr(unsafe.Pointer(&buf[0])), length+1)
	title := syscall.UTF16ToString(buf)
	if strings.Contains(title, "Tech Card Manager") && !strings.Contains(title, "Tray Host") {
		platformRecordAppWindow(hwnd, int(windowPID))
		enumeratedManagerWindow.Store(hwnd)
		return 0
	}
	return 1
})

func trayNotificationCode(lParam uintptr) uint16 {
	return uint16(lParam & 0xffff)
}

func trayIconID(lParam uintptr) uint16 {
	return uint16((lParam >> 16) & 0xffff)
}

func shouldRestoreTrayEvent(event uint16) bool {
	switch event {
	case wmLButtonUp, wmLButtonDblCl, ninSelect, ninKeySelect:
		return true
	default:
		return false
	}
}

var trayWndProc = syscall.NewCallback(func(hwnd uintptr, msg uint32, wParam, lParam uintptr) uintptr {
	switch msg {
	case trayCallback:
		event := trayNotificationCode(lParam)
		if shouldRestoreTrayEvent(event) {
			restoreManagerWindow()
			return 0
		}
		switch event {
		case wmRButtonUp:
			showTrayMenu(hwnd)
			return 0
		}
	case trayRestore:
		restoreManagerWindow()
		return 0
	case wmDestroy:
		deleteTrayIcon()
		signalTrayQuit()
		procPostQuitMsg.Call(0)
		return 0
	}
	ret, _, _ := procDefWindowProc.Call(hwnd, uintptr(msg), wParam, lParam)
	return ret
})

func startWindowsTray(url string) traySignals {
	trayState.Lock()
	if trayState.started {
		trayState.url = url
		signals := traySignals{Quit: trayState.quit, WindowClosed: trayState.windowClosed, ExitRequested: trayState.exitRequested}
		trayState.Unlock()
		return signals
	}
	trayState.started = true
	trayState.url = url
	trayState.quit = make(chan struct{})
	trayState.windowClosed = make(chan struct{}, 1)
	trayState.exitRequested = make(chan struct{}, 1)
	trayState.stopWatch = make(chan struct{})
	signals := traySignals{Quit: trayState.quit, WindowClosed: trayState.windowClosed, ExitRequested: trayState.exitRequested}
	trayState.Unlock()

	go trayMessageLoop()
	return signals
}

func requestExistingManagerRestore() bool {
	className, _ := syscall.UTF16PtrFromString("TechCardManagerTrayHost400")
	windowName, _ := syscall.UTF16PtrFromString("Tech Card Manager Tray Host")
	hwnd, _, _ := procFindWindow.Call(
		uintptr(unsafe.Pointer(className)),
		uintptr(unsafe.Pointer(windowName)),
	)
	if hwnd == 0 {
		return false
	}
	result, _, _ := procPostMessage.Call(hwnd, trayRestore, 0, 0)
	return result != 0
}

func stopWindowsTray() {
	trayState.Lock()
	if !trayState.started {
		trayState.Unlock()
		return
	}
	hwnd := trayState.hostHwnd
	stop := trayState.stopWatch
	trayState.stopWatch = nil
	trayState.Unlock()

	if stop != nil {
		select {
		case <-stop:
		default:
			close(stop)
		}
	}
	deleteTrayIcon()
	if hwnd != 0 {
		procPostMessage.Call(hwnd, wmClose, 0, 0)
	}
}

func trayMessageLoop() {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	// Start ownership monitoring before creating optional tray UI, so even an
	// immediate Edge close or a tray-host creation failure cannot orphan the
	// Manager service in the background.
	go watchManagerWindow()

	className, _ := syscall.UTF16PtrFromString("TechCardManagerTrayHost400")
	windowName, _ := syscall.UTF16PtrFromString("Tech Card Manager Tray Host")
	hInstance, _, _ := procGetModule.Call(0)
	wc := wndClassEx{
		CbSize:        uint32(unsafe.Sizeof(wndClassEx{})),
		LpfnWndProc:   trayWndProc,
		HInstance:     hInstance,
		LpszClassName: className,
	}
	if r, _, e := procRegisterClass.Call(uintptr(unsafe.Pointer(&wc))); r == 0 {
		appendManagerLog(fmt.Sprintf("tray RegisterClassExW: %v", e))
	}
	hwnd, _, e := procCreateWindow.Call(
		0,
		uintptr(unsafe.Pointer(className)),
		uintptr(unsafe.Pointer(windowName)),
		0,
		0, 0, 0, 0,
		0, 0, hInstance, 0,
	)
	if hwnd == 0 {
		appendManagerLog(fmt.Sprintf("tray CreateWindowExW: %v", e))
		return
	}

	trayState.Lock()
	trayState.hostHwnd = hwnd
	trayState.Unlock()
	if err := addTrayIcon(hwnd); err != nil {
		// Do not hide the manager window unless the notification-area icon is
		// actually available; otherwise a minimize could strand the user.
		appendManagerLog("tray icon: " + err.Error())
	}
	// Window lifecycle ownership is independent from notification-area UI.
	// Even if Explorer rejects the tray icon, the watcher above remains active.

	var msg winMsg
	for {
		r, _, _ := procGetMessage.Call(uintptr(unsafe.Pointer(&msg)), 0, 0, 0)
		if int32(r) <= 0 {
			break
		}
		procTranslateMsg.Call(uintptr(unsafe.Pointer(&msg)))
		procDispatchMsg.Call(uintptr(unsafe.Pointer(&msg)))
	}
}

func addTrayIcon(hwnd uintptr) error {
	assetDir := filepath.Join(baseDir(), "assets")
	if err := os.MkdirAll(assetDir, 0755); err != nil {
		return err
	}
	iconPath := filepath.Join(assetDir, "imdb.ico")
	if err := os.WriteFile(iconPath, trayIconBytes, 0644); err != nil {
		return err
	}
	iconPtr, _ := syscall.UTF16PtrFromString(iconPath)
	hIcon, _, e := procLoadImage.Call(0, uintptr(unsafe.Pointer(iconPtr)), imageIcon, 0, 0, lrLoadFromFile|lrDefaultSize)
	if hIcon == 0 {
		return fmt.Errorf("LoadImageW: %v", e)
	}

	nid := notifyIconData{
		HWnd:             hwnd,
		UID:              1,
		UFlags:           nifMessage | nifIcon | nifTip,
		UCallbackMessage: trayCallback,
		HIcon:            hIcon,
	}
	copyUTF16(nid.SzTip[:], "Tech Card Manager")
	nid.CbSize = uint32(unsafe.Sizeof(nid))
	if r, _, e := procShellNotify.Call(nimAdd, uintptr(unsafe.Pointer(&nid))); r == 0 {
		return fmt.Errorf("Shell_NotifyIconW(NIM_ADD): %v", e)
	}
	nid.UVersion = notifyVersion4
	_, _, _ = procShellNotify.Call(nimSetVersion, uintptr(unsafe.Pointer(&nid)))

	trayState.Lock()
	trayState.icon = hIcon
	trayState.iconPath = iconPath
	trayState.trayIconReady = true
	trayState.Unlock()
	return nil
}

func deleteTrayIcon() {
	trayState.Lock()
	hwnd := trayState.hostHwnd
	trayState.trayIconReady = false
	trayState.Unlock()
	if hwnd == 0 {
		return
	}
	nid := notifyIconData{HWnd: hwnd, UID: 1}
	nid.CbSize = uint32(unsafe.Sizeof(nid))
	_, _, _ = procShellNotify.Call(nimDelete, uintptr(unsafe.Pointer(&nid)))
}

func watchManagerWindow() {
	ticker := time.NewTicker(250 * time.Millisecond)
	defer ticker.Stop()
	hadWindow := false
	hadOwnedSession := platformAppWindowSessionActive()
	reportedClosed := false
	for {
		trayState.Lock()
		stop := trayState.stopWatch
		trayState.Unlock()
		select {
		case <-ticker.C:
			hwnd := findManagerWindow()
			if platformAppWindowSessionActive() {
				hadOwnedSession = true
			}
			if hwnd != 0 {
				hadWindow = true
				reportedClosed = false
				trayState.Lock()
				trayState.appHwnd = hwnd
				trayState.Unlock()
				trayState.Lock()
				trayIconReady := trayState.trayIconReady
				trayState.Unlock()
				if trayIconReady && isIconic(hwnd) {
					// A true minimize-to-tray: remove the Edge --app window from
					// the taskbar after Windows has put it in the minimized state.
					procShowWindow.Call(hwnd, swHide)
				}
			} else if (hadWindow || (hadOwnedSession && !platformAppWindowProcessesActive())) && !reportedClosed {
				reportedClosed = true
				trayState.Lock()
				closed := trayState.windowClosed
				exitInProgress := trayState.exitInProgress
				trayState.appHwnd = 0
				trayState.Unlock()
				if closed != nil && !exitInProgress {
					select {
					case closed <- struct{}{}:
					default:
					}
				}
			}
		case <-stop:
			return
		}
	}
}

func platformManagerWindowExists() bool {
	trayState.Lock()
	hwnd := trayState.appHwnd
	trayState.Unlock()
	if isOwnedManagerWindow(hwnd) {
		return true
	}
	return findManagerWindow() != 0
}

func findManagerWindow() uintptr {
	enumeratedManagerWindow.Store(0)
	procEnumWindows.Call(enumManagerWndProc, 0)
	return enumeratedManagerWindow.Load()
}

func isWindow(hwnd uintptr) bool {
	r, _, _ := procIsWindow.Call(hwnd)
	return r != 0
}

func isOwnedManagerWindow(hwnd uintptr) bool {
	if hwnd == 0 || !isWindow(hwnd) {
		return false
	}
	var pid uint32
	procGetWindowPID.Call(hwnd, uintptr(unsafe.Pointer(&pid)))
	return platformOwnsAppWindowPID(int(pid))
}

func isIconic(hwnd uintptr) bool {
	r, _, _ := procIsIconic.Call(hwnd)
	return r != 0
}

func restoreManagerWindow() {
	trayState.Lock()
	hwnd := trayState.appHwnd
	url := trayState.url
	trayState.Unlock()
	if !isOwnedManagerWindow(hwnd) {
		hwnd = findManagerWindow()
	}
	if hwnd == 0 {
		// A UI session may still be starting even before its HWND is visible.
		// Never create a second Edge app window on an impatient tray click.
		if url != "" && !platformAppWindowSessionActive() {
			_ = openAppWindow(url)
		}
		return
	}
	trayState.Lock()
	trayState.appHwnd = hwnd
	trayState.Unlock()
	procShowWindow.Call(hwnd, swShow)
	procShowWindow.Call(hwnd, swRestore)
	procSetForeground.Call(hwnd)
}

func platformExitPromptOwner(windowAlreadyClosed bool) uintptr {
	if !windowAlreadyClosed {
		// A tray-menu exit can originate while the Edge app window is hidden or
		// minimized. Restore it before showing the modal so the question has a
		// visible owner and cannot appear as a minimized background dialog.
		trayState.Lock()
		appHwnd := trayState.appHwnd
		trayState.Unlock()
		if !isOwnedManagerWindow(appHwnd) {
			appHwnd = findManagerWindow()
		}
		if isOwnedManagerWindow(appHwnd) {
			procShowWindow.Call(appHwnd, swShow)
			procShowWindow.Call(appHwnd, swRestore)
			procSetForeground.Call(appHwnd)
			return appHwnd
		}
	}

	// After the title-bar X has already destroyed the Edge HWND, the tray host
	// remains the Manager-owned native window. It is the stable modal owner.
	// Give its message loop a short bounded chance to finish startup in the
	// extremely fast close-after-launch path.
	for attempt := 0; attempt < 25; attempt++ {
		trayState.Lock()
		hostHwnd := trayState.hostHwnd
		trayState.Unlock()
		if hostHwnd != 0 && isWindow(hostHwnd) {
			return hostHwnd
		}
		time.Sleep(20 * time.Millisecond)
	}
	return 0
}

func showTrayMenu(hwnd uintptr) {
	menu, _, _ := procCreateMenu.Call()
	if menu == 0 {
		return
	}
	defer procDestroyMenu.Call(menu)
	openText, _ := syscall.UTF16PtrFromString("打开 Tech Card Manager")
	exitText, _ := syscall.UTF16PtrFromString("退出 Tech Card Manager")
	procAppendMenu.Call(menu, mfString, menuRestore, uintptr(unsafe.Pointer(openText)))
	procAppendMenu.Call(menu, mfString, menuExit, uintptr(unsafe.Pointer(exitText)))
	var pt winPoint
	procGetCursorPos.Call(uintptr(unsafe.Pointer(&pt)))
	procSetForeground.Call(hwnd)
	cmd, _, _ := procTrackPopupMenu.Call(menu, tpmRightButton|tpmReturnCmd, uintptr(pt.X), uintptr(pt.Y), 0, hwnd, 0)
	procPostMessage.Call(hwnd, wmNull, 0, 0)
	switch cmd {
	case menuRestore:
		restoreManagerWindow()
	case menuExit:
		requestTrayExit()
	}
}

func requestTrayExit() {
	trayState.Lock()
	requested := trayState.exitRequested
	trayState.Unlock()
	if requested != nil {
		select {
		case requested <- struct{}{}:
		default:
		}
	}
}

func platformHideManagerWindow() {
	trayState.Lock()
	trayState.exitInProgress = true
	hwnd := trayState.appHwnd
	trayState.Unlock()
	if !isOwnedManagerWindow(hwnd) {
		hwnd = platformOwnedAppWindowHWND()
	}
	if !isOwnedManagerWindow(hwnd) {
		hwnd = findManagerWindow()
	}
	if hwnd != 0 {
		procShowWindow.Call(hwnd, swHide)
	}
}

// platformHideManagerWindowToTray is used only for an explicit login launch.
// It never changes exit state and fails visible if the tray icon or owned Edge
// window cannot be proven ready within the bounded startup window.
func platformHideManagerWindowToTray() bool {
	for attempt := 0; attempt < 100; attempt++ {
		trayState.Lock()
		ready := trayState.trayIconReady
		hwnd := trayState.appHwnd
		trayState.Unlock()
		if ready {
			if !isOwnedManagerWindow(hwnd) {
				hwnd = findManagerWindow()
			}
			if isOwnedManagerWindow(hwnd) {
				procShowWindow.Call(hwnd, swHide)
				return true
			}
		}
		time.Sleep(50 * time.Millisecond)
	}
	appendManagerLog("silent login start skipped: tray or owned window was not ready")
	return false
}

func platformCancelManagerWindowExit() {
	trayState.Lock()
	trayState.exitInProgress = false
	trayState.Unlock()
}

func platformRequestCloseManagerWindow() {
	trayState.Lock()
	hwnd := trayState.appHwnd
	trayState.Unlock()
	if !isOwnedManagerWindow(hwnd) {
		hwnd = platformOwnedAppWindowHWND()
	}
	if !isOwnedManagerWindow(hwnd) {
		hwnd = findManagerWindow()
	}
	if hwnd != 0 {
		procPostMessage.Call(hwnd, wmClose, 0, 0)
	}
}

func signalTrayQuit() {
	trayState.Lock()
	q := trayState.quit
	trayState.Unlock()
	trayState.quitOnce.Do(func() {
		if q != nil {
			close(q)
		}
	})
}

func copyUTF16(dst []uint16, s string) {
	u := syscall.StringToUTF16(s)
	if len(u) > len(dst) {
		u = u[:len(dst)]
		u[len(u)-1] = 0
	}
	copy(dst, u)
}
