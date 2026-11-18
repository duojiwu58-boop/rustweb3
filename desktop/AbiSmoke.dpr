program AbiSmoke;

{$APPTYPE CONSOLE}

uses
  Winapi.Windows, System.SysUtils, WalletApi in 'units\WalletApi.pas';

procedure Require(Value: Boolean; const MessageText: string);
begin
  if not Value then raise Exception.Create(MessageText);
end;

function AwaitTask(Api: TWalletApi; Task: UInt64; out Text: string): Int32;
var
  Deadline: UInt64;
begin
  Deadline := GetTickCount64 + 5000;
  repeat
    Result := Api.Poll(Task, Text);
    if Result <> W3_PENDING then Exit;
    Require(GetTickCount64 < Deadline, 'Polling timeout');
    Sleep(5);
  until False;
end;

var
  Api: TWalletApi;
  Tasks: array[0..15] of UInt64;
  Task, Context: UInt64;
  Text: string;
  Data: PByte;
  Len: UInt32;
  Code, I: Integer;
  Deadline: UInt64;
begin
  try
    Api := TWalletApi.Create(ExtractFilePath(ParamStr(0)));
    try
      Require(Api.Version() = 1, 'Version mismatch');
      for I := 0 to High(Tasks) do
        if Odd(I) then Tasks[I] := Api.Submit('signer.ping')
        else Tasks[I] := Api.Submit('network.ping');
      for I := 0 to High(Tasks) do
      begin
        CheckW3(AwaitTask(Api, Tasks[I], Text));
        Require(Pos(#$6A21#$62DF, Text) = 1, 'UTF-8 Chinese decoding failed');
        Require(Api.Poll(Tasks[I], Text) = -3, 'Consumed task still available');
      end;
      Writeln('PASS concurrent signer/network IPC and UTF-8');

      Task := Api.Submit('signer.ping');
      Api.Cancel(Task);
      Require(AwaitTask(Api, Task, Text) = W3_CANCELLED, 'Cancel failed');
      Task := Api.Submit('network.ping');
      Api.Lock;
      Require(AwaitTask(Api, Task, Text) = W3_CANCELLED, 'Lock failed');
      Writeln('PASS cancellation and lock');

      Task := Api.Submit('network.ping');
      Deadline := GetTickCount64 + 5000;
      repeat
        Code := Api.PollTask(Api.Context, Task, Data, Len);
        if Code <> W3_PENDING then Break;
        Require(GetTickCount64 < Deadline, 'Raw poll timeout');
        Sleep(5);
      until False;
      CheckW3(Code);
      Require(Api.FreeBuffer(Data, Len + 1) = -1, 'Wrong buffer length accepted');
      CheckW3(Api.FreeBuffer(Data, Len));
      Require(Api.FreeBuffer(Data, Len) = -1, 'Double free accepted');
      CheckW3(Api.FreeBuffer(nil, 0));
      Require(Api.SubmitTask(Api.Context, nil, 1, Task) = -1, 'Null accepted');
      Require(Api.CreateContext(nil, 1, Context) = -1, 'Null path accepted');
      Require(Context = 0, 'Failed context not cleared');
      Writeln('PASS allocation ownership and invalid arguments');

      for I := 0 to High(Tasks) do Tasks[I] := Api.Submit('network.ping');
    finally
      Api.Free; // Outstanding tasks must stop before FreeLibrary.
    end;
    for I := 1 to 10 do
    begin
      Api := TWalletApi.Create(ExtractFilePath(ParamStr(0)));
      Api.Submit('signer.ping');
      Api.Free;
    end;
    Writeln('PASS destroy with active tasks and repeated DLL unload');
    Writeln('ABI SMOKE PASSED');
  except
    on E: Exception do
    begin
      Writeln(ErrOutput, E.Message);
      ExitCode := 1;
    end;
  end;
end.
