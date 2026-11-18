unit SecureWalletApi;

interface
uses Winapi.Windows, System.SysUtils, System.Classes;
type
  EWalletRequest = class(Exception)
  public Code: Int32; constructor CreateCode(Value: Int32); end;
  TSecureOpen = function: Int32; cdecl;
  TSecureClose = function: Int32; cdecl;
  TSecureEpoch = function: UInt64; cdecl;
  TSecureExchange = function(Epoch: UInt64; Op: UInt32; Data: PByte; Len: UInt32; Output: PByte;
    Capacity: UInt32; out Written: UInt32): Int32; cdecl;
  TSecureWalletApi = class
  private
    FLib: HMODULE;
    FOpen: TSecureOpen;
    FClose: TSecureClose;
    FExchange: TSecureExchange;
    FGetEpoch: TSecureEpoch;
    FEpoch: UInt64;
    FNetOpen: TSecureOpen;
    FNetClose: TSecureClose;
    FNetExchange: TSecureExchange;
    function Exchange(Call: TSecureExchange; Epoch: UInt64; Op: UInt32; const Fields: array of UTF8String; PublicData: Boolean): UTF8String;
  public
    constructor Create;
    destructor Destroy; override;
    procedure Lock;
    function Request(Op: UInt32; const Fields: array of UTF8String): UTF8String;
    function PublicRequest(Op: UInt32; const Fields: array of UTF8String): UTF8String;
    function RequestAt(Epoch: UInt64; Op: UInt32; const Fields: array of UTF8String): UTF8String;
    property Epoch: UInt64 read FEpoch;
  end;
procedure Wipe(var Value: UTF8String);

implementation
constructor EWalletRequest.CreateCode(Value: Int32);
var Text: string;
begin
  Code := Value;
  case Value of
    -31: Text := '输入格式不正确，或备份单词与这个钱包不一致。请核对助记词及顺序。';
    -32: Text := '本机密码不正确，请重新输入。';
    -33, -35: Text := '备份会话已关闭或超时。重新输入本机密码后，可继续确认备份。';
    -36: Text := '这个钱包已完成备份，请输入密码并显示收款地址。';
    -37: Text := '这个钱包尚未确认备份，请先完成备份确认。';
    -38: Text := '这个钱包正在被其他操作使用，请稍后重试。';
    -39: Text := '这个钱包名称已存在，请打开已有钱包，或换一个新名称。';
    -34: Text := '本机钱包或交易记录无法读写。请保留现有文件，不要删除后重试。';
    -4: Text := '会话已锁定，请重新输入密码。';
    else Text := '钱包连接中断，请关闭并重新打开程序。';
  end;
  inherited CreateFmt('%s (%d)', [Text, Value]);
end;
procedure SecureZeroMemory(Data: Pointer; Count: NativeUInt);
begin
  if Count > 0 then FillChar(Data^, Count, 0);
end;
procedure Wipe(var Value: UTF8String);
begin
  UniqueString(AnsiString(Value));
  if Length(Value) > 0 then SecureZeroMemory(PAnsiChar(Value), Length(Value));
  Value := '';
end;
constructor TSecureWalletApi.Create;
var Code: Int32;
begin
  inherited;
  FLib := LoadLibraryEx(PChar(ExtractFilePath(ParamStr(0)) + 'web3rust.dll'), 0, $100 or $800);
  if FLib = 0 then RaiseLastOSError;
  FOpen := TSecureOpen(GetProcAddress(FLib, 'w3_secure_open'));
  FClose := TSecureClose(GetProcAddress(FLib, 'w3_secure_close'));
  FExchange := TSecureExchange(GetProcAddress(FLib, 'w3_secure_exchange'));
  FGetEpoch := TSecureEpoch(GetProcAddress(FLib, 'w3_secure_epoch'));
  FNetOpen := TSecureOpen(GetProcAddress(FLib, 'w3_network_open'));
  FNetClose := TSecureClose(GetProcAddress(FLib, 'w3_network_close'));
  FNetExchange := TSecureExchange(GetProcAddress(FLib, 'w3_network_exchange'));
  if not Assigned(FOpen) or not Assigned(FClose) or not Assigned(FExchange) or
    not Assigned(FNetOpen) or not Assigned(FNetClose) or not Assigned(FNetExchange) or not Assigned(FGetEpoch) then
    raise Exception.Create('Secure API unavailable');
  Code := FOpen();
  if Code <> 0 then raise Exception.CreateFmt('Isolated session unavailable (%d)', [Code]);
  FEpoch := FGetEpoch();
  Code := FNetOpen();
  if Code <> 0 then raise Exception.CreateFmt('Public channel unavailable (%d)', [Code]);
end;
destructor TSecureWalletApi.Destroy;
begin
  if Assigned(FClose) then FClose();
  if Assigned(FNetClose) then FNetClose();
  if FLib <> 0 then FreeLibrary(FLib);
  inherited;
end;
procedure TSecureWalletApi.Lock;
begin
  if Assigned(FClose) then FClose();
  if FOpen() <> 0 then raise Exception.Create('Could not reopen locked session');
  FEpoch := FGetEpoch();
end;
function TSecureWalletApi.Request(Op: UInt32; const Fields: array of UTF8String): UTF8String;
begin
  Result := Exchange(FExchange, FEpoch, Op, Fields, False);
end;
function TSecureWalletApi.RequestAt(Epoch: UInt64; Op: UInt32; const Fields: array of UTF8String): UTF8String;
begin
  Result := Exchange(FExchange, Epoch, Op, Fields, False);
end;
function TSecureWalletApi.PublicRequest(Op: UInt32; const Fields: array of UTF8String): UTF8String;
begin
  // The offline VCL probe uses stdout for its report, not a network broker.
  if SameText(ParamStr(1),'--self-test') then
    raise Exception.Create('Network unavailable in offline self-test');
  Result := Exchange(FNetExchange, 0, Op, Fields, True);
end;
function TSecureWalletApi.Exchange(Call: TSecureExchange; Epoch: UInt64; Op: UInt32; const Fields: array of UTF8String; PublicData: Boolean): UTF8String;
var Input: TBytes; Output: array[0..2047] of Byte; Posn, I, N, Size: Integer;
    Written: UInt32; Code: Int32; Data: PByte;
begin
  Result := ''; Size := 0;
  for I := 0 to High(Fields) do
  begin
    if Length(Fields[I]) > 1024 then raise Exception.Create('Input too long');
    Inc(Size, 2 + Length(Fields[I]));
  end;
  if Size > 2038 then raise Exception.Create('Request too long');
  SetLength(Input, Size); FillChar(Output, SizeOf(Output), 0);
  try
    Posn := 0;
    for I := 0 to High(Fields) do
    begin
      N := Length(Fields[I]); Input[Posn] := Byte(N); Input[Posn + 1] := Byte(N shr 8);
      Inc(Posn, 2);
      if N > 0 then Move(PAnsiChar(Fields[I])^, Input[Posn], N);
      Inc(Posn, N);
    end;
    if Size = 0 then Data := nil else Data := @Input[0];
    Code := Call(Epoch, Op, Data, Size, @Output[0], SizeOf(Output), Written);
    if Written > SizeOf(Output) then raise Exception.Create('Invalid response length');
    SetString(Result, PAnsiChar(@Output[0]), Written);
    if Code <> 0 then
    begin
      if PublicData and (Written > 0) then raise Exception.Create(string(Result));
      raise EWalletRequest.CreateCode(Code);
    end;
  finally
    if Length(Input) > 0 then SecureZeroMemory(@Input[0], Length(Input));
    SecureZeroMemory(@Output[0], SizeOf(Output));
  end;
end;
end.
