unit WalletApi;

interface

uses Winapi.Windows, System.SysUtils;

const
  W3_OK = 0;
  W3_PENDING = 1;
  W3_CANCELLED = -4;

type
  TW3Version = function: UInt32; cdecl;
  TW3Create = function(Path: PByte; Len: UInt32; out Context: UInt64): Int32; cdecl;
  TW3Destroy = function(Context: UInt64): Int32; cdecl;
  TW3Submit = function(Context: UInt64; Data: PByte; Len: UInt32; out Task: UInt64): Int32; cdecl;
  TW3Poll = function(Context, Task: UInt64; out Data: PByte; out Len: UInt32): Int32; cdecl;
  TW3Cancel = function(Context, Task: UInt64): Int32; cdecl;
  TW3Lock = function(Context: UInt64): Int32; cdecl;
  TW3Free = function(Data: PByte; Len: UInt32): Int32; cdecl;
  TW3Preview = function(Asset: UInt32; Recipient: PByte; RecipientLen: UInt32;
    Amount: PByte; AmountLen: UInt32; out Data: PByte; out Len: UInt32): Int32; cdecl;

  TWalletApi = class
  private
    FLibrary: HMODULE;
    FContext: UInt64;
    function Symbol(const Name: AnsiString): Pointer;
  public
    Version: TW3Version;
    CreateContext: TW3Create;
    DestroyContext: TW3Destroy;
    SubmitTask: TW3Submit;
    PollTask: TW3Poll;
    CancelTask: TW3Cancel;
    LockContext: TW3Lock;
    FreeBuffer: TW3Free;
    PreviewTransfer: TW3Preview;
    constructor Create(const Directory: string);
    destructor Destroy; override;
    function Submit(const Command: UTF8String): UInt64;
    function Poll(Task: UInt64; out Text: string): Int32;
    procedure Cancel(Task: UInt64);
    procedure Lock;
    function Preview(Asset: UInt32; const Recipient, Amount: string; out Json: string): Int32;
    property Context: UInt64 read FContext;
  end;

procedure CheckW3(Code: Int32);

implementation

procedure CheckW3(Code: Int32);
begin
  if Code <> W3_OK then
    raise Exception.CreateFmt('Web3Rust error %d', [Code]);
end;

function TWalletApi.Symbol(const Name: AnsiString): Pointer;
begin
  Result := GetProcAddress(FLibrary, PAnsiChar(Name));
  if Result = nil then
    raise Exception.Create('Missing ABI symbol: ' + string(Name));
end;

constructor TWalletApi.Create(const Directory: string);
var
  Path: string;
  Bytes: UTF8String;
begin
  inherited Create;
  Path := ExpandFileName(Directory);
  FLibrary := LoadLibraryEx(PChar(IncludeTrailingPathDelimiter(Path) + 'web3rust.dll'),
    0, $00000100 or $00000800); // DLL_LOAD_DIR | SYSTEM32
  if FLibrary = 0 then RaiseLastOSError;
  Version := TW3Version(Symbol('w3_version'));
  CreateContext := TW3Create(Symbol('w3_context_create'));
  DestroyContext := TW3Destroy(Symbol('w3_context_destroy'));
  SubmitTask := TW3Submit(Symbol('w3_task_submit'));
  PollTask := TW3Poll(Symbol('w3_task_poll'));
  CancelTask := TW3Cancel(Symbol('w3_task_cancel'));
  LockContext := TW3Lock(Symbol('w3_context_lock'));
  FreeBuffer := TW3Free(Symbol('w3_buffer_free'));
  PreviewTransfer := TW3Preview(Symbol('w3_preview_transfer'));
  if Version() <> 1 then raise Exception.Create('Unsupported ABI version');
  Bytes := UTF8String(Path);
  CheckW3(CreateContext(PByte(PAnsiChar(Bytes)), Length(Bytes), FContext));
end;

destructor TWalletApi.Destroy;
var
  SafeToUnload: Boolean;
begin
  SafeToUnload := True;
  if FContext <> 0 then
  begin
    // If joining failed, leave DLL loaded rather than unloading live code.
    SafeToUnload := DestroyContext(FContext) = W3_OK;
    FContext := 0;
  end;
  if (FLibrary <> 0) and SafeToUnload then FreeLibrary(FLibrary);
  inherited;
end;

function TWalletApi.Submit(const Command: UTF8String): UInt64;
begin
  CheckW3(SubmitTask(FContext, PByte(PAnsiChar(Command)), Length(Command), Result));
end;

function TWalletApi.Poll(Task: UInt64; out Text: string): Int32;
var
  Data: PByte;
  Len: UInt32;
  Bytes: UTF8String;
begin
  Text := '';
  Result := PollTask(FContext, Task, Data, Len);
  if Result <> W3_OK then Exit;
  try
    SetString(Bytes, PAnsiChar(Data), Len);
    Text := UTF8ToString(Bytes);
  finally
    CheckW3(FreeBuffer(Data, Len));
  end;
end;

procedure TWalletApi.Cancel(Task: UInt64);
begin
  CheckW3(CancelTask(FContext, Task));
end;

procedure TWalletApi.Lock;
begin
  CheckW3(LockContext(FContext));
end;

function TWalletApi.Preview(Asset: UInt32; const Recipient, Amount: string; out Json: string): Int32;
var
  AddressBytes, AmountBytes, Bytes: UTF8String;
  Data: PByte;
  Len: UInt32;
begin
  Json := '';
  AddressBytes := UTF8String(Recipient);
  AmountBytes := UTF8String(Amount);
  Result := PreviewTransfer(Asset, PByte(PAnsiChar(AddressBytes)), Length(AddressBytes),
    PByte(PAnsiChar(AmountBytes)), Length(AmountBytes), Data, Len);
  if Result <> W3_OK then Exit;
  try
    SetString(Bytes, PAnsiChar(Data), Len);
    Json := UTF8ToString(Bytes);
  finally
    CheckW3(FreeBuffer(Data, Len));
  end;
end;

end.
